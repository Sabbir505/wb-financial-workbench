//! LLM transport layer (F14): the OpenAI-compatible SSE client that talks to
//! a local llama-server. Pure networking — no GPUI, no process management —
//! so the e2e suite can exercise the exact client the desktop app uses.

use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> ChatMessage {
        ChatMessage { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> ChatMessage {
        ChatMessage { role: "assistant".into(), content: content.into() }
    }
    pub fn system(content: impl Into<String>) -> ChatMessage {
        ChatMessage { role: "system".into(), content: content.into() }
    }
}

/// Extract the content delta from one SSE line ("data: {…}" / "data: [DONE]").
pub fn sse_delta(line: &str) -> Option<String> {
    let rest = line.strip_prefix("data:")?.trim();
    if rest == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(rest).ok()?;
    v["choices"][0]["delta"]["content"].as_str().map(String::from)
}

/// Extract an error message from llama.cpp's streaming error payload
/// (`data: {"error": {"message": "…"}}`) — silently dropped otherwise.
pub fn sse_error(line: &str) -> Option<String> {
    let rest = line.strip_prefix("data:")?.trim();
    if rest == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(rest).ok()?;
    let err = &v["error"];
    if err.is_null() {
        return None;
    }
    Some(
        err["message"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| err.to_string()),
    )
}

/// True once llama-server's HTTP listener answers on `port` (503 while the
/// model loads still counts — the socket is up).
pub fn server_listening(port: u16) -> bool {
    match ureq::get(&format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(2))
        .call()
    {
        Ok(_) => true,
        Err(ureq::Error::Status(_, _)) => true,
        Err(_) => false,
    }
}

/// Streaming completion against llama-server's OpenAI-compatible endpoint.
/// Appends deltas to `sink`; returns when [DONE] arrives, the stream ends or
/// `stop` flips.
pub fn chat_stream(
    port: u16,
    messages: &[ChatMessage],
    sink: &mut String,
    stop: &AtomicBool,
) -> Result<(), String> {
    chat_stream_with(port, messages, sink, stop, |_| {})
}

/// Same, with a per-delta flush hook — the UI passes one that appends each
/// delta into its shared buffer so the response streams incrementally instead
/// of appearing all at once after generation.
///
/// A stalled server surfaces as an error after 60s of socket silence (read
/// timeout); server-side error events (`data: {"error": …}`) return `Err`
/// with the server's message.
pub fn chat_stream_with(
    port: u16,
    messages: &[ChatMessage],
    sink: &mut String,
    stop: &AtomicBool,
    mut on_delta: impl FnMut(&str),
) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let body = serde_json::json!({ "messages": messages, "stream": true });
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .build();
    let resp = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(resp.into_reader());
    let mut line = String::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if let Some(delta) = sse_delta(trimmed) {
            sink.push_str(&delta);
            on_delta(&delta);
        }
        if let Some(err) = sse_error(trimmed) {
            return Err(err);
        }
        if trimmed.trim_end() == "data: [DONE]" {
            break;
        }
    }
    Ok(())
}

/// Shared buffer between the streaming thread and the UI drain loop.
pub struct StreamShared {
    pub inner: Mutex<StreamInner>,
    pub stop: AtomicBool,
}

#[derive(Default)]
pub struct StreamInner {
    pub partial: String,
    pub done: bool,
    pub error: Option<String>,
}

impl StreamShared {
    pub fn new() -> Arc<StreamShared> {
        Arc::new(StreamShared {
            inner: Mutex::new(StreamInner::default()),
            stop: AtomicBool::new(false),
        })
    }
    pub fn snapshot(&self) -> StreamInner {
        self.inner
            .lock()
            .map(|g| StreamInner {
                partial: g.partial.clone(),
                done: g.done,
                error: g.error.clone(),
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parses_deltas_and_done() {
        assert_eq!(
            sse_delta(r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#),
            Some("你好".to_string())
        );
        // role-only chunk carries no content
        assert_eq!(sse_delta(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#), None);
        assert_eq!(sse_delta("data: [DONE]"), None);
        assert_eq!(sse_delta(": keep-alive"), None);
    }

    #[test]
    fn messages_serialize_with_role_and_content() {
        let m = ChatMessage::system("只依据证据作答");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "system");
        assert_eq!(v["content"], "只依据证据作答");
    }
}
