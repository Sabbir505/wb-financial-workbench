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

use wb_core::analytics;
use wb_core::audit::AuditLog;
use wb_core::binding::{
    facts_from_sources, BindingDimension, BindingVerdict, FactSources,
};
use wb_core::guardrail::{check_with_facts, Guardrail};
use wb_core::structural;
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
/// evidence excerpt + structured evidence facts + the vault brief's CALC.
fn app_guardrail(client: &ResearchClient, vault: &Vault, task_id: &str) -> Guardrail {
    let context = client.get_context(task_id).expect("get_context");
    let evidence = client.get_evidence(task_id).expect("get_evidence");
    let detail = client.get_task(task_id).expect("get_task");
    let brief = vault.read_brief(task_id);
    let calc = brief.as_ref().map(|b| b.calc_values());
    let metric_unit = if context.unit.trim().is_empty() {
        "亿元"
    } else {
        context.unit.as_str()
    };
    let facts = facts_from_sources(FactSources {
        metrics: Some(&context.metrics.0),
        metric_unit,
        summary: Some(context.announcement.summary.as_str()),
        evidence: Some(&evidence),
        calc: calc.as_ref(),
        fact_text: None,
    });
    check_with_facts(&detail.sections.inference, &facts, &Default::default())
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
    // 7 fixture tasks: 5 that reached a brief + 2 sectionless terminal states
    // (WAITING_DATA, IGNORED) covering the replay scenarios in spec §2.4c.
    assert_eq!(queue.len(), 7, "7 fixture tasks: {queue:?}");
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

    // --- F4 guardrail flags the two untraceable figures: the model-invented
    // difference (1652) and the construction growth rate (200.71) that is not
    // in the evidence pack. Everything else binds to a real fact.
    let g = app_guardrail(&fx.client, &fx.vault, SDIC);
    assert!(!g.ok(), "untraceable figures must be flagged; got {:?}", g.violations);
    assert_eq!(g.violation_tokens(), vec!["200.71", "1652"]);
    assert_eq!(g.binding_mismatch_count(), 0, "{:?}", g.bindings);

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

/// Binding over the real HTTP evidence pack: a number can be traceable in
/// value and still fail the account, period, or scope test. The value verdict
/// stays green (`ok()` is true), and the binding verdict is the amber second
/// result. This is the path the professor's ontology feeds.
#[test]
fn binding_flags_right_number_wrong_account_period_and_scope() {
    let fx = setup();
    let context = fx.client.get_context(SDIC).expect("get_context");
    let evidence = fx.client.get_evidence(SDIC).expect("get_evidence");
    let brief = fx.vault.read_brief(SDIC);
    let calc = brief.as_ref().map(|b| b.calc_values());
    let metric_unit = if context.unit.trim().is_empty() {
        "亿元"
    } else {
        context.unit.as_str()
    };
    let facts = facts_from_sources(FactSources {
        metrics: Some(&context.metrics.0),
        metric_unit,
        summary: Some(context.announcement.summary.as_str()),
        evidence: Some(&evidence),
        calc: calc.as_ref(),
        fact_text: None,
    });
    let cfg = Default::default();

    // Correct: 24.83亿元 is 2026M9 consolidated revenue.
    let g = check_with_facts("2026年前三季度营业收入24.83亿元", &facts, &cfg);
    assert!(g.ok(), "value must trace: {:?}", g.violations);
    assert_eq!(g.binding_mismatch_count(), 0, "{:?}", g.bindings);
    assert!(g.binding_counts().0 >= 1);

    // Right number, wrong account: 5042.06万元 is deducted profit, not
    // attributable profit.
    let g = check_with_facts("2026年前三季度归母净利润5042.06万元", &facts, &cfg);
    assert!(g.ok(), "the value is traceable: {:?}", g.violations);
    assert_eq!(g.binding_mismatch_count(), 1, "{:?}", g.bindings);
    assert!(matches!(
        g.binding_mismatches().next().unwrap().verdict,
        BindingVerdict::Mismatch {
            dimension: BindingDimension::Metric,
            ..
        }
    ));

    // Right number, wrong period: 24.83亿元 is 2026M9, not 2026Q2.
    let g = check_with_facts("2026Q2营业收入24.83亿元", &facts, &cfg);
    assert!(g.ok());
    assert_eq!(g.binding_mismatch_count(), 1, "{:?}", g.bindings);
    assert!(matches!(
        g.binding_mismatches().next().unwrap().verdict,
        BindingVerdict::Mismatch {
            dimension: BindingDimension::Period,
            ..
        }
    ));

    // Right number, wrong scope: revenue is consolidated, not parent-only.
    let g = check_with_facts("母公司营业收入24.83亿元", &facts, &cfg);
    assert!(g.ok());
    assert_eq!(g.binding_mismatch_count(), 1, "{:?}", g.bindings);
    assert!(matches!(
        g.binding_mismatches().next().unwrap().verdict,
        BindingVerdict::Mismatch {
            dimension: BindingDimension::Scope,
            ..
        }
    ));
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
    let revised_inference = "经营变化：前三季营收19.31亿元（+10.29%），归母净利润0.31亿元（-61.27%），净利率1.62%。\n风险点：盈利大幅回落，扣非净利润669.78万元，盈利质量承压。\n待核实问题：四季度订单与价格走势；产能利用率恢复情况。";
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

/// The preserved draft must survive a second revise.
///
/// Service state resets to WAITING_REVIEW on restart while the vault keeps the
/// revised file. A second approve-as-revised therefore starts from already-
/// revised text; if the backup write is unconditional it overwrites the original
/// `{task}.v1.md` and F6's central promise (the original draft is preserved)
/// silently fails. This is not hypothetical — it happened while running the app
/// selfcheck twice against a live mock.
#[test]
fn second_revise_does_not_clobber_the_preserved_original() {
    let fx = setup();

    let revised_inference = "经营变化：前三季营收19.31亿元（+10.29%），归母净利润0.31亿元（-61.27%），净利率1.62%。\n风险点：盈利大幅回落，扣非净利润669.78万元，盈利质量承压。\n待核实问题：四季度订单与价格走势；产能利用率恢复情况。";

    let backup = fx
        .vault
        .find_brief_path(REVISE_TARGET)
        .expect("brief path")
        .with_file_name(format!("{REVISE_TARGET}.v1.md"));

    // First revise: writes v2, preserves v1 with the violation intact.
    let req = ReviewRequest {
        task_id: REVISE_TARGET.into(),
        verdict: Verdict::ApproveAsRevised,
        comment: Some("first".into()),
        reviewer: "张研".into(),
        idempotency_key: format!("{REVISE_TARGET}:first"),
        revised_sections: Some(
            [("inference", revised_inference)]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
    };
    fx.client.submit_review(&req, true).expect("first revise");
    let first = std::fs::read_to_string(&backup).expect("v1 preserved");
    assert!(first.contains("2460"), "v1 holds the original violation");

    // Reset service state only — the vault keeps the revised file. This is what
    // a mock restart does.
    let reset = format!("{}/__test/reset", fx.server.base_url);
    assert_eq!(post_json(&reset, "{}"), 200);

    // Second revise over the same preserved file.
    let req2 = ReviewRequest {
        task_id: REVISE_TARGET.into(),
        verdict: Verdict::ApproveAsRevised,
        comment: Some("second".into()),
        reviewer: "张研".into(),
        idempotency_key: format!("{REVISE_TARGET}:second"),
        revised_sections: Some(
            [("inference", revised_inference)]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
    };
    let _ = fx.client.submit_review(&req2, true);

    let second = std::fs::read_to_string(&backup).expect("v1 still present");
    assert!(
        second.contains("2460"),
        "the preserved original must not be overwritten by revised text: {second}"
    );
    assert_eq!(first, second, "v1 is immutable once written");
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

/// This session's additions, end-to-end over the real mock HTTP pipeline:
/// F4 layer 2 (structural accounting checks), and F13 (the audit record now
/// carries guardrail tokens + original/revised drafts, and analytics
/// classifies the revision without any model in the loop).
#[test]
fn f13_f4_structural_and_analytics_pipeline() {
    let fx = setup();

    // --- F4 layer 2: structural checks over the task's own CALC values
    let detail = fx.client.get_task(REVISE_TARGET).unwrap();
    let report = structural::check(&detail.sections.calculation.0, &Default::default());
    assert!(
        !report.checked.is_empty(),
        "the fixture's CALC section must trigger at least one rule"
    );
    assert!(
        report.ok(),
        "real fixture numbers must be coherent: {:?}",
        report.violations
    );

    // --- F4 layer 1: guardrail flags the model-invented 差额
    let g = app_guardrail(&fx.client, &fx.vault, REVISE_TARGET);
    assert_eq!(g.violation_tokens(), vec!["2460"]);

    // --- F13 write path: record_review_ex, exactly as workspace.submit_review
    let original_inference = detail.sections.inference.clone();
    let revised_inference = "经营变化：前三季营收19.31亿元（+10.29%），归母净利润0.31亿元（-61.27%），净利率1.62%。
风险点：盈利大幅回落，扣非净利润669.78万元，盈利质量承压。
待核实问题：四季度订单与价格走势；产能利用率恢复情况。";
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
    assert_eq!(receipt.status, "SUCCEEDED");
    fx.audit
        .record_review_ex(
            &req,
            "永冠新材",
            "2026Q4",
            &receipt,
            &g.violation_tokens(),
            Some(original_inference.clone()),
            Some(revised_inference.to_string()),
        )
        .expect("audit append");

    // The audit record carries the review-assist context.
    let records = fx.audit.list();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].guardrail_tokens, vec!["2460"]);
    assert!(records[0]
        .original_inference
        .as_deref()
        .unwrap_or_default()
        .contains("2460"));
    assert!(records[0]
        .revised_inference
        .as_deref()
        .unwrap_or_default()
        .contains("669.78"));
    // Raw JSONL round-trips the new fields for future analytics.
    let raw = std::fs::read_to_string(fx.audit.path()).unwrap();
    assert!(raw.contains("\"guardrail_tokens\":[\"2460\"]"), "{raw}");
    assert!(raw.contains("\"original_inference\""), "{raw}");

    // --- F13 read path: deterministic taxonomy over the recorded revision
    let summary = analytics::analyze(&records);
    assert_eq!(summary.total_reviews, 1);
    assert_eq!(summary.approves, 0);
    assert_eq!(summary.revisions, 1);
    assert!((summary.revision_rate - 1.0).abs() < 1e-9);
    assert!((summary.first_pass_rate).abs() < 1e-9);
    assert_eq!(summary.guardrail_flagged, 1);
    assert_eq!(summary.guardrail_confirmed, 1);
    assert_eq!(summary.top_tokens, vec![("2460".to_string(), 1)]);
    let hallucinated = summary
        .taxonomy
        .iter()
        .find(|(c, _)| *c == analytics::ErrorCategory::HallucinatedNumber)
        .unwrap();
    assert_eq!(hallucinated.1, 1, "2460 removed in revision → hallucinated-number");
    assert_eq!(summary.per_company[0].company, "永冠新材");
    assert_eq!(summary.per_company[0].reviews, 1);
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
