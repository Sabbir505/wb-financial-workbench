//! F8: generic research-service channel. MCP tools/call (I3) is the single write
//! path; read-only graph queries (I4/I6) go over plain HTTP GET. Every call has a
//! timeout and structured errors; write calls respect the approval policy.

use crate::model::{
    ConflictBody, ContextBundle, Evidence, Methodology, ReviewReceipt, ReviewRequest, Stock,
    TaskDetail, TaskSummary,
};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ResearchError {
    #[error("对象不存在: {0}")]
    NotFound(String),
    #[error("状态冲突: {message} (当前状态 {current_status})")]
    Conflict {
        message: String,
        current_status: String,
    },
    #[error("数据缺失: {0}")]
    DataMissing(String),
    #[error("参数错误: {0}")]
    BadRequest(String),
    #[error("请求超时")]
    Timeout,
    #[error("通道不可用: {0}")]
    Transport(String),
}

impl ResearchError {
    pub fn kind(&self) -> &'static str {
        match self {
            ResearchError::NotFound(_) => "404",
            ResearchError::Conflict { .. } => "409",
            ResearchError::DataMissing(_) => "503",
            ResearchError::BadRequest(_) => "400",
            ResearchError::Timeout => "TIMEOUT",
            ResearchError::Transport(_) => "TRANSPORT",
        }
    }
}

/// F8 approval policy: write-path tool calls (submit_review) require explicit
/// user approval; the review dialog is that approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// Writes proceed once the caller passes approved=true (dialog confirmed).
    RequireApproval,
    /// CI / e2e convenience: writes allowed without the flag.
    AllowAll,
}

pub struct ResearchClient {
    api_base: String,
    mcp_url: String,
    agent: ureq::Agent,
    approval: ApprovalPolicy,
}

impl ResearchClient {
    pub fn new(
        api_base: impl Into<String>,
        mcp_url: impl Into<String>,
        timeout: Duration,
        approval: ApprovalPolicy,
    ) -> ResearchClient {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        ResearchClient {
            api_base: api_base.into().trim_end_matches('/').to_string(),
            mcp_url: mcp_url.into(),
            agent,
            approval,
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    // ------------------------------------------------------------------ MCP (I3)

    fn mcp_call(
        &self,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ResearchError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": tool, "arguments": args}
        });
        let resp = self
            .agent
            .post(&self.mcp_url)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| self.map_ureq(e))?;
        let text = resp
            .into_string()
            .map_err(|e| ResearchError::Transport(e.to_string()))?;
        let envelope: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| ResearchError::Transport(format!("bad JSON-RPC envelope: {e}")))?;
        if let Some(err) = envelope.get("error") {
            return Err(ResearchError::BadRequest(format!("{err}")));
        }
        let result = envelope
            .get("result")
            .cloned()
            .ok_or_else(|| ResearchError::Transport("missing result".into()))?;
        if result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let msg = result
                .pointer("/content/0/text")
                .and_then(|v| v.as_str())
                .unwrap_or("tool error");
            return Err(ResearchError::BadRequest(msg.to_string()));
        }
        let content = result
            .pointer("/content/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("null");
        let parsed: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| ResearchError::Transport(format!("tool payload not JSON: {e}")))?;

        // Error-convention payloads inside tool results: {"ok": false, "error": ...}
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            if parsed.get("error").and_then(|v| v.as_str()) == Some("conflict") {
                return Err(ResearchError::Conflict {
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("conflict")
                        .to_string(),
                    current_status: parsed
                        .get("current_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                });
            }
            let message = parsed
                .get("message")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| parsed.to_string());
            if let Some(status) = parsed.get("status").and_then(|v| v.as_u64()) {
                return Err(match status {
                    404 => ResearchError::NotFound(message),
                    503 => ResearchError::DataMissing(message),
                    400 => ResearchError::BadRequest(message),
                    _ => ResearchError::Transport(message),
                });
            }
            return Err(ResearchError::BadRequest(message));
        }
        Ok(parsed)
    }

    fn map_ureq(&self, e: ureq::Error) -> ResearchError {
        match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                match code {
                    404 => ResearchError::NotFound(body),
                    400 => ResearchError::BadRequest(body),
                    503 => ResearchError::DataMissing(body),
                    409 => {
                        let parsed: Result<ConflictBody, _> = serde_json::from_str(&body);
                        match parsed {
                            Ok(c) => ResearchError::Conflict {
                                message: c.message,
                                current_status: c.current_status,
                            },
                            Err(_) => ResearchError::Conflict {
                                message: body,
                                current_status: String::new(),
                            },
                        }
                    }
                    _ => ResearchError::Transport(format!("HTTP {code}: {body}")),
                }
            }
            ureq::Error::Transport(t) => {
                if t.kind() == ureq::ErrorKind::Io && t.to_string().contains("timed out") {
                    ResearchError::Timeout
                } else {
                    ResearchError::Transport(t.to_string())
                }
            }
        }
    }

    /// Queue summary. `status` filters (e.g. WAITING_REVIEW).
    pub fn list_pending(&self, status: Option<&str>) -> Result<Vec<TaskSummary>, ResearchError> {
        let mut args = serde_json::Map::new();
        if let Some(s) = status {
            args.insert("status".into(), serde_json::Value::String(s.into()));
        }
        let v = self.mcp_call("list_pending", serde_json::Value::Object(args))?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("list_pending shape: {e}")))
    }

    pub fn get_task(&self, task_id: &str) -> Result<TaskDetail, ResearchError> {
        let v = self.mcp_call("get_task", serde_json::json!({"task_id": task_id}))?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("get_task shape: {e}")))
    }

    pub fn get_context(&self, task_id: &str) -> Result<ContextBundle, ResearchError> {
        let v = self.mcp_call("get_context", serde_json::json!({"task_id": task_id}))?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("get_context shape: {e}")))
    }

    /// The review write path. `approved` must be true under
    /// ApprovalPolicy::RequireApproval (the review dialog supplies it).
    pub fn submit_review(
        &self,
        req: &ReviewRequest,
        approved: bool,
    ) -> Result<ReviewReceipt, ResearchError> {
        if self.approval == ApprovalPolicy::RequireApproval && !approved {
            return Err(ResearchError::BadRequest(
                "写路径需要人工确认（approval policy）".into(),
            ));
        }
        let v = self.mcp_call("submit_review", serde_json::to_value(req).unwrap())?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("submit_review shape: {e}")))
    }

    // ------------------------------------------------------------- read-only (I4/I6)

    fn get_json(&self, path: &str) -> Result<serde_json::Value, ResearchError> {
        let url = format!("{}{}", self.api_base, path);
        let resp = self.agent.get(&url).call().map_err(|e| self.map_ureq(e))?;
        let text = resp
            .into_string()
            .map_err(|e| ResearchError::Transport(e.to_string()))?;
        serde_json::from_str(&text)
            .map_err(|e| ResearchError::Transport(format!("bad JSON from {path}: {e}")))
    }

    pub fn get_evidence(&self, task_id: &str) -> Result<Evidence, ResearchError> {
        let v = self.get_json(&format!("/audit/{task_id}/evidence"))?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("evidence shape: {e}")))
    }

    pub fn get_methodology(&self, task_id: &str) -> Result<Methodology, ResearchError> {
        let v = self.get_json(&format!("/audit/{task_id}/methodology"))?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("methodology shape: {e}")))
    }

    pub fn stocks_search(&self, q: &str) -> Result<Vec<Stock>, ResearchError> {
        let v = self.get_json(&format!("/stocks/search?q={}", urlencode(q)))?;
        serde_json::from_value(v)
            .map_err(|e| ResearchError::Transport(format!("stocks shape: {e}")))
    }

    /// F10 liveness probe.
    pub fn system_status(&self) -> Result<serde_json::Value, ResearchError> {
        self.get_json("/system/status")
    }

    pub fn health(&self) -> Result<serde_json::Value, ResearchError> {
        self.get_json("/health")
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
