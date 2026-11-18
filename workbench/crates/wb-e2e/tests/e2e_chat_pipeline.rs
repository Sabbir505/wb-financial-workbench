//! End-to-end chat pipeline (F14): the real `wb_core::llm` SSE client against
//! a fake llama-server served over real TCP — no engine binary needed. Covers
//! the wire contract the desktop app depends on: POST /v1/chat/completions
//! with the system-context injection traveling intact, delta assembly, the
//! stop flag, and the /health probe semantics.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::sync::mpsc;
use wb_core::llm::{chat_stream, chat_stream_with, server_listening, ChatMessage, StreamShared};

struct FakeLlama {
    port: u16,
    /// Raw request bodies received on /v1/chat/completions.
    bodies: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
}

/// Spawn a minimal OpenAI-compatible SSE server on an ephemeral port.
fn spawn_fake_llama(deltas: Vec<&'static str>, delay_between: Duration) -> FakeLlama {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let shutdown = Arc::new(AtomicBool::new(false));

    let bodies_t = bodies.clone();
    let shutdown_t = shutdown.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown_t.load(Ordering::Relaxed) {
                break;
            }
            let Ok(mut s) = stream else { continue };
            let bodies_c = bodies_t.clone();
            let deltas: Vec<String> = deltas.iter().map(|d| d.to_string()).collect();
            let delay = delay_between;
            std::thread::spawn(move || {
                let (path, body) = read_request(&mut s);
                match path {
                    p if p.contains("/health") => {
                        let _ = s.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}",
                        );
                    }
                    p if p.contains("/v1/chat/completions") => {
                        bodies_c.lock().unwrap().push(body);
                        let _ = s.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                        );
                        for d in &deltas {
                            let chunk = format!(
                                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
                                d
                            );
                            let _ = s.write_all(chunk.as_bytes());
                            let _ = s.flush();
                            if !delay.is_zero() {
                                std::thread::sleep(delay);
                            }
                        }
                        let _ = s.write_all(b"data: [DONE]\n\n");
                        let _ = s.flush();
                    }
                    _ => {
                        let _ = s.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
                let _ = s.flush();
                // Connection: close — dropping the stream ends the body.
            });
        }
    });

    FakeLlama { port, bodies, shutdown }
}

impl Drop for FakeLlama {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Nudge the accept loop so it observes the flag.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Read one HTTP request (head + Content-Length body); returns (path, body).
fn read_request(s: &mut TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // Head: until \r\n\r\n.
    while !buf.ends_with(b"\r\n\r\n") {
        if s.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let path = head
        .lines()
        .next()
        .unwrap_or("")
        .split(' ')
        .nth(1)
        .unwrap_or("")
        .to_string();
    // Body by Content-Length.
    let mut body = String::new();
    for line in head.lines() {
        if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
            let n: usize = v.trim().parse().unwrap_or(0);
            let mut chunk = vec![0u8; n];
            if s.read(&mut chunk).unwrap_or(0) > 0 {
                body = String::from_utf8_lossy(&chunk).to_string();
            }
        }
    }
    (path, body)
}

#[test]
fn chat_stream_assembles_deltas_and_sends_context() {
    let fake = spawn_fake_llama(vec!["扣非", "净利润", "5042.06万元"], Duration::ZERO);

    let messages = vec![
        ChatMessage::system("你是 WB 审核助手。只依据证据作答，绝不编造数字。"),
        ChatMessage::user("为什么 1652 被护栏拦截？"),
    ];
    let mut sink = String::new();
    let stop = AtomicBool::new(false);
    chat_stream(fake.port, &messages, &mut sink, &stop).expect("stream completes");

    assert_eq!(sink, "扣非净利润5042.06万元");

    // The wire contract: system context + user turn + stream flag travel intact.
    let bodies = fake.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("\"stream\":true"), "{}", bodies[0]);
    assert!(bodies[0].contains("\"role\":\"system\""), "{}", bodies[0]);
    assert!(bodies[0].contains("绝不编造数字"), "{}", bodies[0]);
    assert!(bodies[0].contains("1652"), "{}", bodies[0]);
}

#[test]
fn health_probe_matches_llama_semantics() {
    let fake = spawn_fake_llama(Vec::new(), Duration::ZERO);
    assert!(server_listening(fake.port), "200 /health → listening");

    // Nothing bound here → not listening.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = dead.local_addr().unwrap().port();
    drop(dead);
    assert!(!server_listening(dead_port), "closed port → not listening");
}

#[test]
fn chat_stream_surfaces_transport_errors() {
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    let stop = AtomicBool::new(false);
    let res = chat_stream(port, &[ChatMessage::user("hi")], &mut String::new(), &stop);
    assert!(res.is_err(), "dead endpoint must surface an error, got {res:?}");
}

#[test]
fn stop_flag_ends_a_slow_stream_early() {
    // Second delta arrives long after we set the stop flag.
    let fake = spawn_fake_llama(vec!["第一段", "第二段"], Duration::from_millis(1500));
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        stop2.store(true, Ordering::Relaxed);
    });
    let started = std::time::Instant::now();
    let mut sink = String::new();
    chat_stream(fake.port, &[ChatMessage::user("hi")], &mut sink, &stop)
        .expect("stop must end the stream gracefully, not error");
    assert!(sink.starts_with("第一段"), "partial content kept: {sink}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "stop must cut the wait short, took {:?}",
        started.elapsed()
    );
}

#[test]
fn deltas_flush_incrementally_not_at_the_end() {
    // The P1 regression guard: on_delta must fire per delta as it arrives —
    // the first flush lands BEFORE the server has even written the second.
    let fake = spawn_fake_llama(vec!["第一", "第二"], Duration::from_millis(500));
    let (tx, rx) = mpsc::channel();
    let mut sink = String::new();
    let stop = AtomicBool::new(false);
    let started = std::time::Instant::now();
    chat_stream_with(fake.port, &[ChatMessage::user("hi")], &mut sink, &stop, move |_| {
        let _ = tx.send(started.elapsed());
    })
    .expect("stream completes");
    let mut times: Vec<std::time::Duration> = rx.try_iter().collect();
    assert_eq!(times.len(), 2, "one flush per delta");
    times.sort();
    assert!(
        times[0] < Duration::from_millis(400),
        "first delta must flush immediately, took {times:?}"
    );
    assert!(started.elapsed() >= Duration::from_millis(500));
}

#[test]
fn server_error_events_surface_as_err() {
    // llama.cpp streaming error payload: data: {"error": {"message": …}}.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let _ = read_request(&mut s);
        let _ = s.write_all(
            b"HTTP/1.1 200 OK
Content-Type: text/event-stream
Connection: close

",
        );
        let _ = s.write_all(
            b"data: {\"error\":{\"message\":\"model failed to load\"}}

",
        );
        let _ = s.flush();
    });
    let stop = AtomicBool::new(false);
    let res = chat_stream(port, &[ChatMessage::user("hi")], &mut String::new(), &stop);
    let err = res.expect_err("server error must surface");
    assert!(err.contains("model failed to load"), "{err}");
}

#[test]
fn stream_shared_buffers_thread_to_ui() {
    // The thread→UI handoff: deltas accumulate under the mutex, the snapshot
    // carries them, done/error flags land once.
    let shared = StreamShared::new();
    {
        let mut g = shared.inner.lock().unwrap();
        g.partial.push_str("斯迪克");
        g.done = true;
    }
    let snap = shared.snapshot();
    assert_eq!(snap.partial, "斯迪克");
    assert!(snap.done);
    assert!(snap.error.is_none());
}
