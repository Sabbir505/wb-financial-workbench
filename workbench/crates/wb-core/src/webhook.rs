//! I1: new-brief notification webhook. WB runs a tiny HTTP listener; the research
//! service POSTs `{kind: "brief_ready", task_id, ...}` when a brief lands in the
//! vault. The payload is NOT consumed (brief content always comes from the I2
//! file); duplicates are dropped (idempotent — one reminder per task).

use serde::Deserialize;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tiny_http::{Method, Response, Server};

#[derive(Clone, Debug, Deserialize)]
pub struct WebhookEvent {
    #[serde(default)]
    pub kind: String,
    pub task_id: String,
    #[serde(default)]
    pub object_id: String,
    #[serde(default)]
    pub period: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub vault_path: String,
    #[serde(default)]
    pub emitted_at: String,
}

pub struct WebhookListener {
    pub addr: SocketAddr,
    /// True once the socket is bound and the accept loop is running.
    pub ready: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl WebhookListener {
    /// Bind and start the accept loop on a background thread. Events are delivered
    /// via the returned receiver; duplicates by task_id are suppressed.
    pub fn start(port: u16) -> std::io::Result<(WebhookListener, mpsc::Receiver<WebhookEvent>)> {
        let server = Server::http(("127.0.0.1", port)).map_err(std::io::Error::other)?;
        let addr = server
            .server_addr()
            .to_ip()
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], port)));
        let (tx, rx) = mpsc::channel();
        let seen: Arc<Mutex<std::collections::HashSet<String>>> =
            Arc::new(Mutex::new(std::collections::HashSet::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(true)); // socket already bound above
        let stop2 = stop.clone();
        std::thread::Builder::new()
            .name("wb-webhook".into())
            .spawn(move || {
                // Poll with a timeout so `stop()` is observed even without traffic
                // (a plain `incoming_requests()` loop would block forever).
                loop {
                    if stop2.load(Ordering::SeqCst) {
                        break;
                    }
                    let mut request = match server.recv_timeout(IDLE_POLL) {
                        Ok(Some(request)) => request,
                        _ => continue,
                    };
                    match (request.method(), request.url()) {
                        (Method::Post, url) if url.starts_with("/webhook/i1") => {
                            let mut body = String::new();
                            // Cap the read: a runaway body must not buffer unbounded.
                            let _ = request
                                .as_reader()
                                .take(MAX_BODY_BYTES)
                                .read_to_string(&mut body);
                            let handled = match serde_json::from_str::<WebhookEvent>(&body) {
                                Ok(ev) => {
                                    let fresh = seen
                                        .lock()
                                        .map(|mut s| s.insert(ev.task_id.clone()))
                                        .unwrap_or(false);
                                    if fresh {
                                        let _ = tx.send(ev);
                                    }
                                    true
                                }
                                Err(_) => false,
                            };
                            let status = if handled { 204 } else { 400 };
                            let _ = request.respond(Response::empty(status));
                        }
                        (Method::Get, url) if url.starts_with("/health") => {
                            let _ = request.respond(Response::from_string(
                                "{\"ok\":true,\"channel\":\"wb-i1\"}",
                            ));
                        }
                        _ => {
                            let _ = request.respond(Response::empty(404));
                        }
                    }
                }
            })?;
        Ok((WebhookListener { addr, ready, stop }, rx))
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Idle poll interval for the accept loop; also bounds stop() shutdown latency.
const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(200);
/// Upper bound on webhook payload reads (the I1 body is a small JSON object).
const MAX_BODY_BYTES: u64 = 64 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;
    use std::time::Duration;

    fn post(addr: SocketAddr, path: &str, body: &str) -> u16 {
        use std::io::Read;
        let mut s = TcpStream::connect(addr).unwrap();
        write!(
            s,
            "POST {path} HTTP/1.1\r\nHost: t\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        let status_line = buf.lines().next().unwrap().to_string();
        status_line
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn delivers_once_per_task_and_rejects_bad_payloads() {
        let (listener, rx) = WebhookListener::start(0).unwrap();
        assert!(listener.ready.load(Ordering::SeqCst));
        let payload = r#"{"kind":"brief_ready","task_id":"RT-1","title":"斯迪克 2026Q4 财报研究简报","vault_path":"research/2026-11/RT-1.md"}"#;
        assert_eq!(post(listener.addr, "/webhook/i1", payload), 204);
        // duplicate (idempotent)
        assert_eq!(post(listener.addr, "/webhook/i1", payload), 204);
        // malformed
        assert_eq!(post(listener.addr, "/webhook/i1", "{"), 400);

        let ev = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("event delivered");
        assert_eq!(ev.task_id, "RT-1");
        assert_eq!(ev.kind, "brief_ready");
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "duplicate suppressed"
        );
    }
}
