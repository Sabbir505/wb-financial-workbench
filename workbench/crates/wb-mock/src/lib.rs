//! wb-mock: a stand-in KG-Palantir research service for WB development and e2e.
//!
//! Serves the vendored `vendor/api_mock` real-run samples and implements the
//! I3 MCP tools with live state transitions (WAITING_REVIEW → SUCCEEDED /
//! CANCELLED, 409 conflicts, idempotency, 修改后通过 version bumps that write
//! brief v2 into the I2 vault). Test hooks (`/__test/*`) drive webhook emission
//! and state resets from the e2e suite.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server};
use wb_core::model::{Evidence, HistoryEntry, TaskDetail, TaskSummary};
use wb_core::vault::{render_brief_md, Vault};
use wb_core::{model::Frontmatter, state::TaskState};

#[derive(Clone, Debug)]
pub struct MockConfig {
    /// 0 = ephemeral port (e2e).
    pub port: u16,
    /// Directory holding the api_mock JSON samples.
    pub api_mock_dir: PathBuf,
    /// I2 vault root; briefs are seeded here on startup.
    pub vault_root: PathBuf,
    /// Where to forward I1 webhooks when /__test/emit_webhook fires.
    pub webhook_target: Option<String>,
}

impl MockConfig {
    pub fn new(api_mock_dir: impl Into<PathBuf>, vault_root: impl Into<PathBuf>) -> MockConfig {
        MockConfig {
            port: 0,
            api_mock_dir: api_mock_dir.into(),
            vault_root: vault_root.into(),
            webhook_target: None,
        }
    }
}

struct TaskRecord {
    detail: TaskDetail,
    explanation: Value,
    evidence: Evidence,
    methodology: Value,
    priority: &'static str,
    /// Set by 修改后通过; served on the explanation endpoint until reset.
    revised_inference: Option<String>,
}

struct State {
    vault_root: PathBuf,
    tasks: BTreeMap<String, TaskRecord>,
    idempotency: BTreeMap<String, Value>,
    review_counter: usize,
    webhook_target: Option<String>,
    /// Original (v1) vault paths already revised — for /__test/reset restore.
    initial_details: BTreeMap<String, TaskDetail>,
}

pub struct MockServer {
    pub addr: SocketAddr,
    pub base_url: String,
    stop: Arc<AtomicBool>,
    vault_root: PathBuf,
}

impl MockServer {
    /// Load fixtures, seed the vault, bind and start serving.
    pub fn spawn(cfg: MockConfig) -> Result<MockServer, String> {
        let state = Arc::new(Mutex::new(load_state(&cfg)?));
        let server = Server::http(("127.0.0.1", cfg.port)).map_err(|e| e.to_string())?;
        let addr = server
            .server_addr()
            .to_ip()
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], cfg.port)));
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let state2 = state.clone();
        std::thread::Builder::new()
            .name("wb-mock".into())
            .spawn(move || loop {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                match server.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(Some(request)) => handle(&state2, request),
                    _ => continue,
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(MockServer {
            addr,
            base_url: format!("http://{addr}"),
            stop,
            vault_root: cfg.vault_root,
        })
    }

    pub fn vault_root(&self) -> &Path {
        &self.vault_root
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

fn read_json(dir: &Path, name: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(dir.join(name))
        .map_err(|e| format!("missing fixture {name}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("bad fixture {name}: {e}"))
}

/// The 修改后通过 target: the real 永冠新材 fixture (S603681) whose v1 inference
/// carries the model-invented difference "约2460万" (0.312934 − 0.066978 ≈ 0.246亿).
/// Per the revise sample, the task returns to WAITING_REVIEW for human re-review.
fn rework_failed_as_reviewable(mut detail: TaskDetail) -> TaskDetail {
    detail.status = "WAITING_REVIEW".into();
    detail.history.push(HistoryEntry {
        at: detail.updated_at.clone(),
        from: Some("FAILED".into()),
        to: "WAITING_REVIEW".into(),
        note: "数字校验未通过转人工：重新提交人工审核（护栏复检）".into(),
    });
    detail
}

fn load_state(cfg: &MockConfig) -> Result<State, String> {
    let dir = &cfg.api_mock_dir;
    let mut tasks: BTreeMap<String, TaskRecord> = BTreeMap::new();

    let put = |detail: TaskDetail,
               priority: &'static str,
               explanation: Value,
               tasks: &mut BTreeMap<String, TaskRecord>| {
        let tid = detail.task_id.clone();
        let evidence = if tid == "RT-PR-EV-20261029-1225636931-S300806" {
            // The main storyline ships a real evidence pack (raw-yuan excerpt).
            serde_json::from_value(read_json(dir, "audit_evidence.response.json")?)
                .unwrap_or_else(|_| synthesize_evidence(&detail))
        } else {
            synthesize_evidence(&detail)
        };
        let mut methodology = read_json(dir, "audit_methodology.response.json")?;
        methodology["task_id"] = json!(tid.clone());
        tasks.insert(
            tid,
            TaskRecord {
                detail,
                explanation,
                evidence,
                methodology,
                priority,
                revised_inference: None,
            },
        );
        Ok::<(), String>(())
    };

    // S300806 斯迪克 — the main storyline (WAITING_REVIEW, guardrail case 1652万)
    let sdic: TaskDetail = serde_json::from_value(read_json(dir, "analysis_task.response.json")?)
        .map_err(|e| e.to_string())?;
    let sdic_expl = read_json(dir, "analysis_explanation.response.json")?;
    put(sdic, "P1", sdic_expl, &mut tasks)?;

    // Static WAITING_REVIEW samples without dedicated explanation files.
    for (file, priority) in [
        ("analysis_task.fenghua.json", "P1"),
        ("analysis_task.jiemei.json", "P2"),
    ] {
        let detail: TaskDetail =
            serde_json::from_value(read_json(dir, file)?).map_err(|e| e.to_string())?;
        let expl = synthesize_explanation(&detail, true);
        put(detail, priority, expl, &mut tasks)?;
    }
    // 永冠新材: the revise sample's target — FAILED at numeric check, re-submitted
    // to human review with the v1 violation ("约2460万") intact.
    {
        let detail: TaskDetail =
            serde_json::from_value(read_json(dir, "analysis_task.yongguan_failed.json")?)
                .map_err(|e| e.to_string())?;
        let detail = rework_failed_as_reviewable(detail);
        let expl = synthesize_explanation(&detail, false);
        put(detail, "P2", expl, &mut tasks)?;
    }
    // Static FAILED sample (stays FAILED → exercises the 409 path).
    {
        let detail: TaskDetail =
            serde_json::from_value(read_json(dir, "analysis_task.luxshare_failed.json")?)
                .map_err(|e| e.to_string())?;
        let expl = read_json(dir, "analysis_explanation.luxshare_failed.json")?;
        put(detail, "P3", expl, &mut tasks)?;
    }

    // Normalize brief coordinates: every task points at its I2 vault file at v1.
    for rec in tasks.values_mut() {
        if rec.detail.object_name.is_empty() {
            // Fixtures without object_name: derive from "永冠新材：上海永冠…" fact line.
            rec.detail.object_name = rec
                .detail
                .sections
                .fact
                .0
                .first()
                .and_then(|l| l.split('：').next())
                .unwrap_or(&rec.detail.object_id)
                .to_string();
        }
        let ym = rec.detail.created_at.get(..7).unwrap_or("1970-01");
        rec.detail.brief_path = Some(format!("research/{ym}/{}.md", rec.detail.task_id));
        rec.detail.brief_version.get_or_insert(1);
    }

    // Seed the I2 vault so F1/F2 work without the research service writing first.
    let vault = Vault::new(&cfg.vault_root);
    let sdic_brief = std::fs::read_to_string(dir.join("i2_brief_sample.md")).unwrap_or_default();
    for (tid, rec) in &tasks {
        let path = vault.find_brief_path(tid);
        if path.is_some() {
            continue;
        }
        let md = if tid == "RT-PR-EV-20261029-1225636931-S300806" && !sdic_brief.is_empty() {
            sdic_brief.clone()
        } else {
            render_task_brief(rec)
        };
        vault
            .write_brief(&md, tid, &rec.detail.created_at)
            .map_err(|e| format!("vault seed failed: {e}"))?;
    }

    Ok(State {
        vault_root: cfg.vault_root.clone(),
        initial_details: tasks
            .iter()
            .map(|(k, v)| (k.clone(), v.detail.clone()))
            .collect(),
        tasks,
        idempotency: BTreeMap::new(),
        review_counter: 0,
        webhook_target: cfg.webhook_target.clone(),
    })
}

fn frontmatter_of(rec: &TaskRecord) -> Frontmatter {
    let d = &rec.detail;
    Frontmatter {
        task_id: d.task_id.clone(),
        trace_id: d.trace_id.clone(),
        event_ref: event_ref_of(&d.trace_id),
        object_id: d.object_id.clone(),
        object_version: object_version_of(&d.trace_id),
        period: d.period.clone(),
        workflow_version: d.workflow_version.clone(),
        status: fm_status(&d.status),
        review_comment: String::new(),
        reviewer: String::new(),
        created_at: d.created_at.clone(),
        data_cutoff: d.data_cutoff.clone(),
        source_version: d.source_version.clone(),
    }
}

/// `PR-EV-20261029-1225636931-S300806` → `EV-20261029-1225636931`: the event
/// reference drops the `PR-` channel prefix and the trailing `-S<object>` id
/// (matches the I2 sample brief and the `EV-*` fixture event ids).
fn event_ref_of(trace_id: &str) -> String {
    let parts: Vec<&str> = trace_id.split('-').collect();
    if parts.len() >= 4 && parts[0] == "PR" {
        parts[1..parts.len() - 1].join("-")
    } else {
        trace_id.to_string()
    }
}

/// `PR-EV-20261029-1225636931-S300806` → `20261029-1225636931`.
fn object_version_of(trace_id: &str) -> String {
    let parts: Vec<&str> = trace_id.split('-').collect();
    if parts.len() >= 4 && parts[0] == "PR" {
        parts[2..parts.len() - 1].join("-")
    } else {
        trace_id.to_string()
    }
}

fn fm_status(status: &str) -> String {
    match status {
        "WAITING_REVIEW" => "待审核",
        "SUCCEEDED" => "已通过",
        "CANCELLED" => "已驳回",
        "FAILED" => "已失败",
        _ => "进行中",
    }
    .to_string()
}

fn render_task_brief(rec: &TaskRecord) -> String {
    let d = &rec.detail;
    let title = format!("{} {} 财报研究简报", d.object_name, d.period);
    render_brief_md(&frontmatter_of(rec), &title, &d.sections)
}

fn synthesize_evidence(detail: &TaskDetail) -> Evidence {
    let url = detail
        .sections
        .fact
        .0
        .iter()
        .find_map(|l| {
            l.split("：")
                .nth(1)
                .and_then(|s| s.trim().strip_prefix("http").map(|_| s.trim().to_string()))
        })
        .unwrap_or_else(|| {
            format!(
                "http://static.cninfo.com.cn/finalpage/mock/{}.PDF",
                detail.task_id
            )
        });
    let mut parts = Vec::new();
    for (k, v) in &detail.sections.calculation.0 {
        if let Some(n) = v.as_f64() {
            parts.push(format!("{k} {n}"));
        }
    }
    Evidence {
        event_id: event_ref_of(&detail.trace_id),
        source_url: url,
        excerpt: parts.join("；"),
        locator: "PDF 第 2 页「一、主要财务数据（一）主要会计数据和财务指标」".into(),
        fetched_at: detail.created_at.clone(),
    }
}

fn synthesize_explanation(detail: &TaskDetail, ok: bool) -> Value {
    json!({
        "task_id": detail.task_id,
        "explanation": {
            "inference_text": detail.sections.inference,
            "numeric_check": {
                "ok": ok,
                "violations": if ok { vec![] } else { vec![0.24] },
                "checked_values": [],
                "tolerance_pct": 1.0
            },
            "generated_by": "kimi-k3",
            "discipline": "模型只生成可被数字校验的叙述；方向性结论不来自模型"
        }
    })
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

fn json_response(request: Request, code: u16, body: Value) {
    let payload = serde_json::to_string(&body).unwrap_or_default();
    let header = Header::from_bytes(
        &b"Content-Type"[..],
        &b"application/json; charset=utf-8"[..],
    )
    .unwrap();
    let _ = request.respond(
        Response::from_string(payload)
            .with_status_code(code)
            .with_header(header),
    );
}

fn handle(state: &Arc<Mutex<State>>, mut request: Request) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url.clone(), String::new()),
    };
    let mut body = String::new();
    if method == Method::Post {
        let _ = request.as_reader().read_to_string(&mut body);
    }
    let body_json: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or(Value::Null)
    };

    match (&method, path.as_str()) {
        (Method::Post, "/mcp") => handle_mcp(state, body_json, request),
        (Method::Get, p) if p.starts_with("/api/analysis/history") => {
            let s = state.lock().unwrap();
            let filter = query_param(&query, "status");
            let list: Vec<TaskSummary> = queue(&s, filter.as_deref());
            json_response(request, 200, serde_json::to_value(list).unwrap());
        }
        (Method::Get, p) if p.starts_with("/api/analysis/") && p.ends_with("/explanation") => {
            let tid = p
                .trim_start_matches("/api/analysis/")
                .trim_end_matches("/explanation");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => {
                    let mut expl = rec.explanation.clone();
                    if let Some(rev) = &rec.revised_inference {
                        expl["explanation"]["inference_text"] = json!(rev);
                        expl["explanation"]["numeric_check"] = json!({"ok": true, "violations": [], "checked_values": [], "tolerance_pct": 1.0});
                    }
                    json_response(request, 200, expl);
                }
                None => json_response(
                    request,
                    404,
                    json!({"ok": false, "status": 404, "message": "task not found"}),
                ),
            }
        }
        (Method::Get, p) if p.starts_with("/api/analysis/") => {
            let tid = p.trim_start_matches("/api/analysis/");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => {
                    json_response(request, 200, serde_json::to_value(&rec.detail).unwrap())
                }
                None => json_response(
                    request,
                    404,
                    json!({"ok": false, "status": 404, "message": "task not found"}),
                ),
            }
        }
        (Method::Get, p) if p.starts_with("/api/audit/") && p.ends_with("/evidence") => {
            let tid = p
                .trim_start_matches("/api/audit/")
                .trim_end_matches("/evidence");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => {
                    json_response(request, 200, serde_json::to_value(&rec.evidence).unwrap())
                }
                None => json_response(
                    request,
                    404,
                    json!({"ok": false, "status": 404, "message": "task not found"}),
                ),
            }
        }
        (Method::Get, p) if p.starts_with("/api/audit/") && p.ends_with("/methodology") => {
            let tid = p
                .trim_start_matches("/api/audit/")
                .trim_end_matches("/methodology");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => json_response(request, 200, rec.methodology.clone()),
                None => json_response(
                    request,
                    404,
                    json!({"ok": false, "status": 404, "message": "task not found"}),
                ),
            }
        }
        (Method::Get, p) if p.starts_with("/api/audit/") => {
            let tid = p.trim_start_matches("/api/audit/");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => json_response(
                    request,
                    200,
                    json!({
                        "task_id": tid,
                        "status": rec.detail.status,
                        "history": rec.detail.history,
                        "audit_chain": {"ok": true, "length": rec.detail.history.len()}
                    }),
                ),
                None => json_response(
                    request,
                    404,
                    json!({"ok": false, "status": 404, "message": "task not found"}),
                ),
            }
        }
        (Method::Get, p) if p.starts_with("/api/stocks/search") => {
            let s = state.lock().unwrap();
            // The WB client percent-encodes q (Chinese names); decode before matching.
            let q = percent_decode(&query_param(&query, "q").unwrap_or_default());
            let mut list: Vec<Value> = Vec::new();
            for rec in s.tasks.values() {
                let d = &rec.detail;
                if q.is_empty()
                    || d.object_name.contains(&q)
                    || d.object_id.contains(&q)
                    || d.object_id.trim_start_matches('S').contains(&q)
                {
                    list.push(json!({
                        "object_id": d.object_id, "name": d.object_name,
                        "ticker": d.object_id.trim_start_matches('S'),
                        "exchange": "SZ", "sector": "功能涂层材料（锚）", "listed": true
                    }));
                }
            }
            json_response(request, 200, json!(list));
        }
        (Method::Get, "/api/system/status")
        | (Method::Get, "/api/health")
        | (Method::Get, "/health") => {
            let s = state.lock().unwrap();
            let waiting = s
                .tasks
                .values()
                .filter(|r| r.detail.status == "WAITING_REVIEW")
                .count();
            let failed = s
                .tasks
                .values()
                .filter(|r| r.detail.status == "FAILED")
                .count();
            json_response(
                request,
                200,
                json!({
                    "ok": true,
                    "uptime_hours": 26.5,
                    "stores": {"anchor": {"events": 57, "predictions": 64, "backtests": 40}, "icg": {"events": 105, "predictions": 118, "backtests": 68}},
                    "research": {"waiting_review": waiting, "context_ready": 7, "failed": failed},
                    "last_pipeline_run": {"anchor": "2026-11-02T07:12:00", "morning": "2026-11-02T06:57:00"}
                }),
            );
        }
        (Method::Post, "/__test/emit_webhook") => {
            let task_id = body_json
                .get("task_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            let s = state.lock().unwrap();
            let Some(tid) = task_id.or_else(|| {
                s.tasks
                    .values()
                    .find(|r| r.detail.status == "WAITING_REVIEW")
                    .map(|r| r.detail.task_id.clone())
            }) else {
                drop(s);
                json_response(request, 404, json!({"ok": false, "message": "no task"}));
                return;
            };
            let target = s.webhook_target.clone();
            let payload = match s.tasks.get(&tid) {
                Some(rec) => json!({
                    "kind": "brief_ready",
                    "task_id": tid,
                    "object_id": rec.detail.object_id,
                    "period": rec.detail.period,
                    "title": format!("{} {} 财报研究简报", rec.detail.object_name, rec.detail.period),
                    "vault_path": rec.detail.brief_path.clone().unwrap_or_default(),
                    "emitted_at": now_iso()
                }),
                None => {
                    drop(s);
                    json_response(
                        request,
                        404,
                        json!({"ok": false, "message": "unknown task_id"}),
                    );
                    return;
                }
            };
            drop(s);
            let Some(target) = target else {
                json_response(
                    request,
                    503,
                    json!({"ok": false, "message": "no webhook target configured"}),
                );
                return;
            };
            match forward_webhook(&target, &payload) {
                Ok(code) => json_response(
                    request,
                    200,
                    json!({"ok": true, "forwarded": code, "task_id": tid}),
                ),
                Err(e) => json_response(request, 502, json!({"ok": false, "message": e})),
            }
        }
        (Method::Post, "/__test/reset") => {
            // Reset restores status/history/idempotency in place; the vault keeps
            // whatever files exist (e2e uses fresh tempdirs per test).
            let mut s = state.lock().unwrap();
            for (tid, detail) in s.initial_details.clone() {
                if let Some(rec) = s.tasks.get_mut(&tid) {
                    rec.detail = detail;
                    rec.revised_inference = None;
                }
            }
            s.idempotency.clear();
            s.review_counter = 0;
            json_response(request, 200, json!({"ok": true}));
        }
        _ => json_response(
            request,
            404,
            json!({"ok": false, "message": format!("no route: {method} {path}")}),
        ),
    }
}

fn handle_mcp(state: &Arc<Mutex<State>>, body: Value, request: Request) {
    let name = body
        .pointer("/params/name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let args = body
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or(json!({}));

    let result: Result<Value, Value> = match name.as_str() {
        "list_pending" => {
            let filter = args
                .get("status")
                .and_then(|v| v.as_str())
                .map(String::from);
            let s = state.lock().unwrap();
            let list: Vec<TaskSummary> = queue(&s, filter.as_deref());
            Ok(serde_json::to_value(list).unwrap())
        }
        "get_task" => {
            let tid = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => Ok(serde_json::to_value(&rec.detail).unwrap()),
                None => Err(
                    json!({"ok": false, "error": "not_found", "status": 404, "message": format!("task not found: {tid}")}),
                ),
            }
        }
        "get_context" => {
            let tid = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let s = state.lock().unwrap();
            match s.tasks.get(tid) {
                Some(rec) => Ok(build_context(rec)),
                None => Err(
                    json!({"ok": false, "error": "not_found", "status": 404, "message": format!("task not found: {tid}")}),
                ),
            }
        }
        "submit_review" => submit_review(state, &args),
        other => Err(
            json!({"ok": false, "error": "unknown_tool", "status": 400, "message": format!("unknown tool: {other}")}),
        ),
    };

    // Both Ok payloads and the error-convention payloads ride inside a normal
    // JSON-RPC result (isError stays false); the client inspects `ok`/`error`.
    let payload = match result {
        Ok(payload) | Err(payload) => payload,
    };
    let envelope = json!({
        "jsonrpc": "2.0", "id": body.get("id").cloned().unwrap_or(json!(1)),
        "result": {"content": [{"type": "text", "text": serde_json::to_string(&payload).unwrap()}], "isError": false}
    });
    json_response(request, 200, envelope);
}

fn build_context(rec: &TaskRecord) -> Value {
    let d = &rec.detail;
    let fact_title = d.sections.fact.0.first().cloned().unwrap_or_default();
    let title = fact_title
        .split('（')
        .next()
        .unwrap_or(&fact_title)
        .to_string();
    let summary = d
        .sections
        .fact
        .0
        .iter()
        .find(|l| l.starts_with("摘要："))
        .map(|l| l.trim_start_matches("摘要：").to_string())
        .unwrap_or_default();
    let source_url = d
        .sections
        .fact
        .0
        .iter()
        .find(|l| l.starts_with("原文："))
        .map(|l| l.trim_start_matches("原文：").trim().to_string())
        .unwrap_or_default();
    let event_date = d
        .sections
        .fact
        .0
        .first()
        .and_then(|l| l.rsplit('（').next())
        .and_then(|s| s.trim_end_matches('）').to_string().into())
        .unwrap_or_else(|| "2026-10-29".to_string());
    let pct_change = d
        .sections
        .calculation
        .0
        .get("pct_change_event")
        .and_then(|v| v.as_f64());
    json!({
        "task_id": d.task_id,
        "data_cutoff": d.data_cutoff,
        "source_version": d.source_version,
        "announcement": {
            "event_id": event_ref_of(&d.trace_id),
            "title": title,
            "event_date": event_date,
            "event_type": "earnings",
            "source_url": source_url,
            "summary": summary,
            "pct_change": pct_change
        },
        "company": {
            "object_id": d.object_id,
            "name": d.object_name,
            "ticker": d.object_id.trim_start_matches('S'),
            "exchange": "SZ",
            "sector": "功能涂层材料（锚）"
        },
        "metrics": serde_json::to_value(&d.sections.calculation).unwrap(),
        "_comment": "ContextBundle 为固化快照：简报生成后不可变，任何复核/回放都还原同一份证据"
    })
}

fn submit_review(state: &Arc<Mutex<State>>, args: &Value) -> Result<Value, Value> {
    let tid = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let verdict = args
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let reviewer = args
        .get("reviewer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let comment = args
        .get("comment")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let key = args
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let revised = args.get("revised_sections").cloned();

    let mut s = state.lock().unwrap();
    if !key.is_empty() {
        if let Some(receipt) = s.idempotency.get(&key) {
            return Ok(receipt.clone());
        }
    }
    // Pre-validate with an immutable borrow, then apply the transition.
    {
        let Some(rec) = s.tasks.get(&tid) else {
            return Err(
                json!({"ok": false, "error": "not_found", "status": 404, "message": format!("task not found: {tid}")}),
            );
        };
        if rec.detail.status != "WAITING_REVIEW" {
            return Err(json!({
                "ok": false, "error": "conflict",
                "message": format!("任务当前状态为 {}，不可审核（需 WAITING_REVIEW）", rec.detail.status),
                "task_id": tid, "current_status": rec.detail.status
            }));
        }
    }
    let revised_inference: Option<String> = if verdict == "修改后通过" {
        let inference = revised
            .as_ref()
            .and_then(|r| r.get("inference"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if inference.trim().is_empty() {
            return Err(
                json!({"ok": false, "error": "bad_request", "status": 400, "message": "修改后通过需要非空 revised_sections.inference 修改稿"}),
            );
        }
        Some(inference)
    } else if verdict != "通过" && verdict != "驳回" {
        return Err(
            json!({"ok": false, "error": "bad_request", "status": 400, "message": format!("未知 verdict: {verdict}")}),
        );
    } else {
        None
    };

    let vault_root = s.vault_root.clone();
    s.review_counter += 1;
    let review_id = format!("RV-20261102-{:04}", s.review_counter);

    let receipt = {
        let rec = s.tasks.get_mut(&tid).expect("existence checked above");
        let new_state = match verdict.as_str() {
            "通过" | "修改后通过" => TaskState::Succeeded,
            "驳回" => TaskState::Cancelled,
            _ => unreachable!("validated above"),
        };
        let mut receipt = json!({
            "ok": true,
            "review_id": review_id,
            "status": new_state.as_str(),
            "trace_id": rec.detail.trace_id,
            "reviewed_at": now_iso()
        });

        let from = rec.detail.status.clone();
        match verdict.as_str() {
            "通过" => {
                rec.detail.history.push(HistoryEntry {
                    at: now_iso(),
                    from: Some(from),
                    to: "SUCCEEDED".into(),
                    note: format!("人工审核通过（{reviewer}）"),
                });
            }
            "驳回" => {
                rec.detail.history.push(HistoryEntry {
                    at: now_iso(),
                    from: Some(from),
                    to: "CANCELLED".into(),
                    note: format!("人工审核驳回：{comment}（{reviewer}）"),
                });
            }
            "修改后通过" => {
                let inference = revised_inference.expect("validated above");
                let old_version = rec.detail.brief_version.unwrap_or(1);
                let new_version = old_version + 1;
                // Persist v2 into the vault (original untouched: kept as .v{n} backup).
                if rec.detail.brief_path.is_some() {
                    let vault = Vault::new(vault_root);
                    if let Some(path) = vault.find_brief_path(&tid) {
                        if let Ok(original) = std::fs::read_to_string(&path) {
                            let backup = path.with_file_name(format!(
                                "{}.v{old_version}.md",
                                path.file_stem().and_then(|st| st.to_str()).unwrap_or(&tid)
                            ));
                            let _ = std::fs::write(&backup, &original);
                            if let Ok(brief) = wb_core::vault::parse_brief(&path) {
                                let mut fm = brief.frontmatter.clone();
                                fm.status = "已通过".into();
                                fm.reviewer = reviewer.clone();
                                fm.review_comment = comment.clone();
                                let sections = wb_core::model::BriefSections {
                                    fact: wb_core::model::FactList(original_fact_lines(&original)),
                                    calculation: calc_from_original(&original),
                                    inference: inference.clone(),
                                    check: format!(
                                        "数字校验通过：修改稿已引用公告原始数字（护栏复检 0 违例）；审核人 {reviewer}；v{new_version}"
                                    ),
                                };
                                let md = render_brief_md(&fm, &brief.title, &sections);
                                let _ = std::fs::write(&path, md);
                            }
                        }
                    }
                }
                rec.detail.brief_version = Some(new_version);
                rec.revised_inference = Some(inference.clone());
                // The task payload reflects the revised (v2) artifacts.
                rec.detail.sections.inference = inference;
                rec.detail.sections.check = format!(
                    "数字校验通过：修改稿已引用公告原始数字（护栏复检 0 违例）；审核人 {reviewer}；v{new_version}"
                );
                rec.detail.history.push(HistoryEntry {
                    at: now_iso(),
                    from: Some(from),
                    to: "SUCCEEDED".into(),
                    note: format!("修改后通过：简报升 v{new_version}，原稿保留可查（{reviewer}）"),
                });
                receipt["brief_version"] = json!(new_version);
                receipt["note"] = json!(format!("原稿（version {old_version}）保留可查，修改稿升 version={new_version} 落 artifacts"));
            }
            _ => unreachable!(),
        }
        rec.detail.status = new_state.as_str().to_string();
        rec.detail.updated_at = now_iso();
        receipt
    };

    if !key.is_empty() {
        s.idempotency.insert(key, receipt.clone());
    }
    Ok(receipt)
}

// The revise writer uses the per-server vault root carried in State.

fn original_fact_lines(original: &str) -> Vec<String> {
    wb_core::vault::parse_brief_str(original, Path::new("x"))
        .ok()
        .and_then(|b| {
            b.section(wb_core::vault::SectionKind::Fact)
                .map(|s| s.body.clone())
        })
        .map(|body| {
            body.lines()
                .map(|l| l.trim_start_matches("- ").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn calc_from_original(original: &str) -> wb_core::model::CalcValues {
    wb_core::vault::parse_brief_str(original, Path::new("x"))
        .map(|b| wb_core::model::CalcValues(from_vault_calc(&b)))
        .unwrap_or_default()
}

fn from_vault_calc(brief: &wb_core::vault::Brief) -> BTreeMap<String, Value> {
    brief
        .calc_values()
        .into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect()
}

fn queue(s: &State, status: Option<&str>) -> Vec<TaskSummary> {
    let mut out: Vec<TaskSummary> = s
        .tasks
        .values()
        .map(|rec| {
            let d = &rec.detail;
            TaskSummary {
                task_id: d.task_id.clone(),
                object_id: d.object_id.clone(),
                object_name: d.object_name.clone(),
                period: d.period.clone(),
                status: d.status.clone(),
                title: format!("{}：{}", d.object_name, d.period),
                priority: rec.priority.to_string(),
                created_at: d.created_at.clone(),
                updated_at: d.updated_at.clone(),
                fail_reason: d.fail_reason.clone(),
            }
        })
        .collect();
    if let Some(st) = status {
        out.retain(|t| t.status == st);
    }
    out.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(b.updated_at.cmp(&a.updated_at))
    });
    out
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// Minimal percent-decoding for query values (`%XX` and `+` as space), the
/// inverse of the client's `urlencode`.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);
static INIT_TIME: std::sync::OnceLock<std::time::SystemTime> = std::sync::OnceLock::new();

fn now_iso() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = INIT_TIME.get_or_init(std::time::SystemTime::now);
    let secs = base
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Rough local ISO timestamp; ordering is what matters for mock history.
    let (y, mo, d, h, mi, s) = civil_from_unix(secs + n as u64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
}

fn civil_from_unix(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days (Howard Hinnant's algorithm), UTC — good enough for mock data.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d, h, mi, s)
}

fn forward_webhook(target: &str, payload: &Value) -> Result<u16, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let resp = agent
        .post(target)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::to_string(payload).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    Ok(resp.status())
}
