//! End-to-end review loop: real HTTP between the WB client and a spawned mock
//! research service, real I2 vault files, real I1 webhook delivery, real
//! guardrail evaluation and local audit logging.
//!
//! Covers the P0 closed loop from WB-Function-Interface-List-final:
//! F1/F2 vault briefs, F4 guardrail, F5 review actions (通过/驳回/修改后通过),
//! F6 version bump, F7 audit trail, F8 MCP channel semantics (404/409/400),
//! F9/I1 webhook (idempotent), F10 degraded service, F11 evidence drill-down,
//! F12 importance×urgency placement.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use wb_core::audit::AuditLog;
use wb_core::guardrail::{build_evidence_numbers, check as guardrail_check, EvidenceInputs};
use wb_core::model::{ReviewRequest, Verdict};
use wb_core::research::{ApprovalPolicy, ResearchClient, ResearchError};
use wb_core::state::{quadrant_of, TaskState};
use wb_core::vault::{SectionKind, Vault};
use wb_core::webhook::{WebhookEvent, WebhookListener};
use wb_mock::{MockConfig, MockServer};

const SDIC: &str = "RT-PR-EV-20261029-1225636931-S300806";
/// 永冠新材 — FAILED at numeric check ("约2460万"), re-submitted for human review.
const REVISE_TARGET: &str = "RT-PR-EV-20261029-1225636631-S603681";
/// 立讯精密 — stays FAILED → exercises the 409 conflict path.
const LUXSHARE_FAILED: &str = "RT-PR-EV-20261029-1225645718-S002475";

struct Fixture {
    _tmp: tempfile::TempDir,
    server: MockServer,
    client: ResearchClient,
    vault: Vault,
    audit: AuditLog,
    webhook_rx: Receiver<WebhookEvent>,
}

fn api_mock_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/api_mock")
}

fn setup() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (listener, rx) = WebhookListener::start(0).expect("webhook listener");
    let cfg = MockConfig {
        port: 0,
        api_mock_dir: api_mock_dir(),
        vault_root: tmp.path().join("vault"),
        webhook_target: Some(format!("http://{}/webhook/i1", listener.addr)),
    };
    let server = MockServer::spawn(cfg).expect("mock server");
    let client = ResearchClient::new(
        format!("{}/api", server.base_url),
        format!("{}/mcp", server.base_url),
        Duration::from_secs(5),
        ApprovalPolicy::RequireApproval,
    );
    let vault = Vault::new(tmp.path().join("vault"));
    let audit = AuditLog::at(tmp.path().join("wb-data").join("reviews.jsonl"));
    // `listener` is dropped on purpose: its accept thread keeps serving; the
    // stop flag is only used on graceful shutdown, which tests don't need.
    std::mem::forget(listener);
    Fixture {
        _tmp: tmp,
        server,
        client,
        vault,
        audit,
        webhook_rx: rx,
    }
}

/// Guardrail exactly as the app builds it: metrics + announcement summary +
/// evidence excerpt + the vault brief's CALCULATION section.
fn app_guardrail(
    client: &ResearchClient,
    vault: &Vault,
    task_id: &str,
) -> wb_core::guardrail::Guardrail {
    let context = client.get_context(task_id).expect("get_context");
    let evidence = client.get_evidence(task_id).expect("get_evidence");
    let detail = client.get_task(task_id).expect("get_task");
    let mut inputs = EvidenceInputs {
        metrics: context
            .metrics
            .0
            .values()
            .filter_map(|v| v.as_f64())
            .collect(),
        free_texts: vec![
            context.announcement.summary.clone(),
            evidence.excerpt.clone(),
        ],
        checked_values: Vec::new(),
    };
    if let Some(brief) = vault.read_brief(task_id) {
        inputs.metrics.extend(brief.calc_values().values().copied());
    }
    guardrail_check(
        &detail.sections.inference,
        &build_evidence_numbers(&inputs),
        &Default::default(),
    )
}

fn post_json(url: &str, body: &str) -> u16 {
    let http = ureq::AgentBuilder::new().build();
    http.post(url).send_string(body).expect("post").status()
}

#[test]
fn system_queue_and_quadrant_contract() {
    let fx = setup();
    let status = fx.client.system_status().expect("system status");
    assert_eq!(status["ok"], serde_json::Value::Bool(true));
    assert!(status["research"]["waiting_review"].as_u64().unwrap() >= 4);

    let queue = fx.client.list_pending(None).expect("list_pending");
    assert_eq!(queue.len(), 5, "5 fixture tasks: {queue:?}");
    let waiting = fx.client.list_pending(Some("WAITING_REVIEW")).unwrap();
    assert_eq!(waiting.len(), 4);
    assert!(waiting.iter().all(|t| t.status == "WAITING_REVIEW"));

    let sdic = queue
        .iter()
        .find(|t| t.task_id == SDIC)
        .expect("sdic in queue");
    assert_eq!(sdic.status, "WAITING_REVIEW");
    assert_eq!(sdic.priority, "P1");
    assert_eq!(sdic.object_name, "斯迪克");
    assert_eq!(sdic.title, "斯迪克：2026Q4");

    // P1 tasks sort first.
    assert_eq!(queue.first().unwrap().priority, "P1");

    let now =
        chrono::NaiveDateTime::parse_from_str("2026-11-02T12:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
    assert_eq!(
        quadrant_of("P1", "2026-11-02T07:17:03", &now),
        0,
        "urgent+important"
    );
    assert_eq!(
        quadrant_of("P2", "2026-10-01T07:17:03", &now),
        1,
        "important, stale"
    );
    assert_eq!(
        quadrant_of("P3", "2026-11-02T07:17:03", &now),
        2,
        "urgent, minor"
    );
    assert_eq!(quadrant_of("P4", "2026-10-01T07:17:03", &now), 3, "neither");
}

#[test]
fn full_review_loop_approve() {
    let fx = setup();

    // --- F11 drill-down sources: detail + context + evidence + methodology
    let detail = fx.client.get_task(SDIC).expect("get_task");
    assert_eq!(detail.status, "WAITING_REVIEW");
    assert_eq!(detail.object_name, "斯迪克");
    assert_eq!(detail.brief_version, Some(1));
    assert_eq!(detail.sections.fact.0.len(), 3);
    assert!(detail.sections.inference.contains("1652"));
    assert!(TaskState::parse(&detail.status).unwrap().is_reviewable());

    let context = fx.client.get_context(SDIC).unwrap();
    assert_eq!(context.company.name, "斯迪克");
    assert_eq!(context.company.ticker, "300806");
    assert_eq!(context.metrics.0["revenue"], serde_json::json!(24.83));
    assert!(context.announcement.summary.contains("24.83亿元"));
    assert!(context.announcement.source_url.starts_with("http"));

    let evidence = fx.client.get_evidence(SDIC).unwrap();
    assert!(evidence.source_url.contains("cninfo.com.cn"));
    assert!(evidence.excerpt.contains("66,937,816.48"));
    assert!(evidence.locator.contains("PDF"));

    let methodology = fx.client.get_methodology(SDIC).unwrap();
    assert_eq!(methodology.pipeline.len(), 5);
    assert!((methodology.validation.tolerance_pct - 1.0).abs() < 1e-9);

    // --- F4 guardrail flags exactly the model-invented difference
    let g = app_guardrail(&fx.client, &fx.vault, SDIC);
    assert!(
        !g.ok(),
        "the 1652万 difference must be flagged; got {:?}",
        g.violations
    );
    assert_eq!(g.violation_tokens(), vec!["1652"]);

    // --- F1/F2 vault brief: 13 frontmatter fields + 4 sections
    let brief = fx.vault.read_brief(SDIC).expect("brief seeded in vault");
    let fm = &brief.frontmatter;
    assert_eq!(fm.task_id, SDIC);
    assert_eq!(fm.trace_id, "PR-EV-20261029-1225636931-S300806");
    assert_eq!(fm.event_ref, "EV-20261029-1225636931");
    assert_eq!(fm.object_id, "S300806");
    assert_eq!(fm.object_version, "20261029-1225636931");
    assert_eq!(fm.period, "2026Q4");
    assert_eq!(fm.status, "待审核");
    assert!(fm.reviewer.is_empty());
    assert!(fm.review_comment.is_empty());
    assert_eq!(fm.created_at, "2026-11-02T07:17:03");
    assert_eq!(fm.data_cutoff, "2026-11-02T07:12:33");
    assert_eq!(fm.source_version, "icg-store@2026-10-31T22:25:45");
    assert_eq!(fm.workflow_version, "wf-earnings-v1.0");
    assert_eq!(brief.sections.len(), 4);
    assert!(brief
        .section(SectionKind::Fact)
        .unwrap()
        .body
        .contains("cninfo"));
    assert!(brief
        .section(SectionKind::Calculation)
        .unwrap()
        .body
        .contains("revenue = 24.83"));
    assert!(brief
        .section(SectionKind::Inference)
        .unwrap()
        .body
        .contains("1652"));
    assert!(brief
        .section(SectionKind::Check)
        .unwrap()
        .body
        .contains("0.1652"));
    let calc = brief.calc_values();
    assert_eq!(calc["revenue"], 24.83);
    assert_eq!(calc["net_profit"], 0.669378);

    // --- F9/I1 webhook: notification → single reminder (idempotent)
    let code = post_json(
        &format!("{}/__test/emit_webhook", fx.server.base_url),
        &format!(r#"{{"task_id":"{SDIC}"}}"#),
    );
    assert_eq!(code, 200);
    let ev: WebhookEvent = fx
        .webhook_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("webhook delivered");
    assert_eq!(ev.kind, "brief_ready");
    assert_eq!(ev.task_id, SDIC);
    assert!(ev.title.contains("斯迪克"));
    let _ = post_json(
        &format!("{}/__test/emit_webhook", fx.server.base_url),
        &format!(r#"{{"task_id":"{SDIC}"}}"#),
    );
    assert!(
        fx.webhook_rx
            .recv_timeout(Duration::from_millis(400))
            .is_err(),
        "duplicate webhook must be suppressed (one reminder per task)"
    );

    // --- F8 approval policy: write path without approval is rejected
    let req = ReviewRequest {
        task_id: SDIC.into(),
        verdict: Verdict::Approve,
        comment: Some("数据与公告原文一致，现金流风险已在推断节标注".into()),
        reviewer: "张研".into(),
        idempotency_key: format!("{SDIC}:v1"),
        revised_sections: None,
    };
    match fx.client.submit_review(&req, false) {
        Err(ResearchError::BadRequest(msg)) => assert!(msg.contains("人工确认")),
        other => panic!("expected approval-policy rejection, got {other:?}"),
    }

    // --- F5 approve → SUCCEEDED, F7 audit record, history entry
    let receipt = fx.client.submit_review(&req, true).expect("submit approve");
    assert!(receipt.ok);
    assert_eq!(receipt.status, "SUCCEEDED");
    assert!(receipt.review_id.starts_with("RV-"));
    assert_eq!(
        receipt.trace_id.as_deref(),
        Some("PR-EV-20261029-1225636931-S300806")
    );

    // F7: the app writes the local audit record from request + receipt.
    fx.audit
        .record_review(&req, "斯迪克", "2026Q4", &receipt)
        .expect("audit append");

    let records = fx.audit.list();
    assert_eq!(records.len(), 1, "F7 local audit trail written");
    assert_eq!(records[0].task_id, SDIC);
    assert_eq!(records[0].verdict, "通过");
    assert_eq!(records[0].status_after, "SUCCEEDED");
    assert_eq!(records[0].review_id, receipt.review_id);
    assert!(!records[0].revised);

    let after = fx.client.get_task(SDIC).unwrap();
    assert_eq!(after.status, "SUCCEEDED");
    let last = after.history.last().unwrap();
    assert_eq!(last.to, "SUCCEEDED");
    assert!(last.note.contains("人工审核通过"));

    // --- F8 error conventions: idempotent replay + 409 conflict
    let receipt2 = fx
        .client
        .submit_review(&req, true)
        .expect("idempotent replay");
    assert_eq!(receipt2.review_id, receipt.review_id);
    let conflicting = ReviewRequest {
        idempotency_key: format!("{SDIC}:v9"),
        ..req
    };
    match fx.client.submit_review(&conflicting, true) {
        Err(ResearchError::Conflict {
            current_status,
            message,
        }) => {
            assert_eq!(current_status, "SUCCEEDED");
            assert!(message.contains("不可审核"));
        }
        other => panic!("expected 409 conflict, got {other:?}"),
    }

    // --- queue reflects the new state; audit export round-trips
    let queue = fx.client.list_pending(Some("WAITING_REVIEW")).unwrap();
    assert!(queue.iter().all(|t| t.task_id != SDIC));
    assert_eq!(
        TaskState::parse(&after.status).unwrap(),
        TaskState::Succeeded
    );
    let dest = fx._tmp.path().join("export").join("reviews.json");
    assert_eq!(fx.audit.export_json(&dest).unwrap(), 1);
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dest).unwrap()).unwrap();
    assert_eq!(exported.as_array().unwrap().len(), 1);
}

#[test]
fn revise_flow_bumps_version_and_clears_violation() {
    let fx = setup();

    // v1 (re-worked 永冠新材 task) carries the model-invented difference 约2460万.
    let detail = fx.client.get_task(REVISE_TARGET).unwrap();
    assert_eq!(detail.status, "WAITING_REVIEW");
    assert_eq!(detail.object_name, "永冠新材");
    assert_eq!(detail.brief_version, Some(1));
    let g = app_guardrail(&fx.client, &fx.vault, REVISE_TARGET);
    assert!(!g.ok(), "2460万 must be flagged; got {:?}", g.violations);
    assert_eq!(g.violation_tokens(), vec!["2460"]);

    // Empty revised content → 400.
    let empty = ReviewRequest {
        task_id: REVISE_TARGET.into(),
        verdict: Verdict::ApproveAsRevised,
        comment: None,
        reviewer: "张研".into(),
        idempotency_key: format!("{REVISE_TARGET}:v1"),
        revised_sections: Some(
            [("inference", "  ")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
    };
    match fx.client.submit_review(&empty, true) {
        Err(ResearchError::BadRequest(m)) => assert!(m.contains("非空")),
        other => panic!("expected 400 for empty revision, got {other:?}"),
    }

    // The corrected rewrite (from i3_submit_review.revise.request.json).
    let revised_inference = "经营变化：前三季营收19.31亿元（+10.29%），归母净利润0.31亿元（-61.27%），净利率1.62%。\n风险点：盈利大幅回落，扣非净利润0.07亿元，盈利质量承压。\n待核实问题：四季度订单与价格走势；产能利用率恢复情况。";
    let req = ReviewRequest {
        task_id: REVISE_TARGET.into(),
        verdict: Verdict::ApproveAsRevised,
        comment: Some("差额表述已按公告原始数字修正".into()),
        reviewer: "张研".into(),
        idempotency_key: format!("{REVISE_TARGET}:v1"),
        revised_sections: Some(
            [("inference", revised_inference)]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
    };
    let receipt = fx.client.submit_review(&req, true).expect("submit revise");
    fx.audit
        .record_review(&req, "永冠新材", "2026Q4", &receipt)
        .expect("audit append");
    assert_eq!(receipt.status, "SUCCEEDED");
    assert_eq!(receipt.brief_version, Some(2));
    assert!(receipt.note.unwrap_or_default().contains("保留可查"));

    // Vault: main brief rewritten to v2 (guardrail-clean), original kept as .v1.
    let updated = fx.vault.read_brief(REVISE_TARGET).expect("v2 brief");
    assert_eq!(updated.frontmatter.status, "已通过");
    assert_eq!(updated.frontmatter.reviewer, "张研");
    assert_eq!(
        updated.frontmatter.review_comment,
        "差额表述已按公告原始数字修正"
    );
    assert_eq!(
        updated.section(SectionKind::Inference).unwrap().body.trim(),
        revised_inference
    );
    assert!(updated
        .section(SectionKind::Check)
        .unwrap()
        .body
        .contains("v2"));

    let backup = updated
        .path
        .with_file_name(format!("{REVISE_TARGET}.v1.md"));
    let original = std::fs::read_to_string(&backup).expect("v1 backup preserved");
    assert!(
        original.contains("2460"),
        "original untouched and still has the violation"
    );

    // The revised text passes the guardrail against its own evidence.
    let g2 = app_guardrail(&fx.client, &fx.vault, REVISE_TARGET);
    assert!(
        g2.ok(),
        "revised inference must be clean: {:?}",
        g2.violations
    );

    // Server state: SUCCEEDED at v2; audit records the revision.
    let after = fx.client.get_task(REVISE_TARGET).unwrap();
    assert_eq!(after.status, "SUCCEEDED");
    assert_eq!(after.brief_version, Some(2));
    assert!(after.history.last().unwrap().note.contains("修改后通过"));
    let records = fx.audit.list();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].verdict, "修改后通过");
    assert!(records[0].revised);
}

#[test]
fn conflict_on_failed_task_and_unknown_object_404() {
    let fx = setup();
    let req = ReviewRequest {
        task_id: LUXSHARE_FAILED.into(),
        verdict: Verdict::Approve,
        comment: None,
        reviewer: "张研".into(),
        idempotency_key: format!("{LUXSHARE_FAILED}:v1"),
        revised_sections: None,
    };
    match fx.client.submit_review(&req, true) {
        Err(ResearchError::Conflict { current_status, .. }) => {
            assert_eq!(current_status, "FAILED")
        }
        other => panic!("expected conflict on FAILED task, got {other:?}"),
    }

    let ghost = ReviewRequest {
        task_id: "RT-DOES-NOT-EXIST".into(),
        verdict: Verdict::Approve,
        comment: None,
        reviewer: "张研".into(),
        idempotency_key: "k".into(),
        revised_sections: None,
    };
    assert!(matches!(
        fx.client.submit_review(&ghost, true),
        Err(ResearchError::NotFound(_))
    ));
    assert!(matches!(
        fx.client.get_task("RT-DOES-NOT-EXIST"),
        Err(ResearchError::NotFound(_))
    ));
    assert!(matches!(
        fx.client.get_context("RT-DOES-NOT-EXIST"),
        Err(ResearchError::NotFound(_))
    ));
    assert!(matches!(
        fx.client.get_evidence("RT-DOES-NOT-EXIST"),
        Err(ResearchError::NotFound(_))
    ));
}

#[test]
fn degraded_service_surfaces_transport_errors() {
    // F10: a dead research service must yield structured Transport errors —
    // the UI maps these to the degraded banner instead of an empty queue.
    let client = ResearchClient::new(
        "http://127.0.0.1:1/api".to_string(),
        "http://127.0.0.1:1/mcp".to_string(),
        Duration::from_millis(1500),
        ApprovalPolicy::RequireApproval,
    );
    assert!(matches!(
        client.list_pending(None),
        Err(ResearchError::Transport(_))
    ));
    assert!(matches!(
        client.system_status(),
        Err(ResearchError::Transport(_))
    ));
}
