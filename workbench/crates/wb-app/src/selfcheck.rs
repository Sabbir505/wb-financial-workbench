//! Self-check drivers: `--selfcheck` verifies the window renders; `--selfcheck-e2e`
//! drives the real GPUI app through the full review loop via the same handler
//! methods the UI buttons invoke (open_task → guardrail → review modal →
//! simulated edit → submit_review), against a live research service.

use crate::workspace::Workspace;
use gpui::{AsyncApp, WindowHandle};
use std::time::Duration;
use wb_core::model::Verdict;

const SDIC: &str = "RT-PR-EV-20261029-1225636931-S300806";

/// The reviewer's corrected rewrite (mirrors i3_submit_review.revise.request.json).
const REVISED_INFERENCE: &str = "经营变化：前三季营收+10.90%、归母净利+47.86%、扣非+98.64%；Q3单季增速明显回落；在建工程+200.71%，扩产项目进入建设期。\n风险点：经营现金流-7483.90万元（-127.02%），存货及应收账款占用增加；净利率约2.7%，盈利偏薄。\n待核实问题：膜材扩产与磁性材料项目投产节奏及产能消化；非经常性损益具体构成；四季度经营现金流能否改善。";

/// Hold durations are env-tunable so external capture scripts can catch the
/// modal / final states without touching test logic.
fn hold_ms(env: &str, default_ms: u64) -> Duration {
    Duration::from_millis(
        std::env::var(env)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default_ms),
    )
}

async fn wait_for(
    handle: &WindowHandle<Workspace>,
    cx: &mut AsyncApp,
    what: &str,
    pred: impl Fn(&Workspace) -> bool + Copy,
) {
    let mut waited = 0u64;
    loop {
        let ok = handle.update(cx, |ws, _w, _| pred(ws)).unwrap_or(false);
        if ok {
            println!("SELFCHECK step ok: {what}");
            return;
        }
        assert!(waited < 15_000, "selfcheck-e2e timeout waiting for: {what}");
        cx.background_executor()
            .timer(Duration::from_millis(200))
            .await;
        waited += 200;
    }
}

pub async fn drive_review_loop(handle: WindowHandle<Workspace>, cx: &mut AsyncApp) {
    // 1. Queue loads from the research service.
    wait_for(&handle, cx, "queue loaded", |ws| {
        !ws.queue.is_empty() && ws.service_ok == Some(true)
    })
    .await;

    // 2. Open the top-priority pending task (same handler as the inbox row click).
    handle
        .update(cx, |ws, _w, cx| ws.open_task(SDIC, cx))
        .expect("open_task");
    wait_for(&handle, cx, "task detail loaded", |ws| {
        ws.detail.as_ref().map(|d| d.task_id.as_str()) == Some(SDIC) && !ws.detail_loading
    })
    .await;
    handle
        .update(cx, |ws, _w, _| {
            let d = ws.detail.as_ref().expect("detail");
            assert_eq!(d.status, "WAITING_REVIEW");
            assert_eq!(d.object_name, "斯迪克");
            // F4 guardrail flagged the model-invented difference in the UI state.
            let g = ws.guardrail.as_ref().expect("guardrail computed on open");
            assert!(!g.ok(), "1652万 must be flagged in the app");
            assert_eq!(g.violation_tokens(), vec!["1652"]);
            // Vault brief reachable for the same task (F1/F2).
            assert!(ws.vault.read_brief(SDIC).is_some(), "brief in vault");
        })
        .expect("assert detail");

    // 3. Open the review modal with 修改后通过 (same handler as the button).
    handle
        .update(cx, |ws, _w, cx| {
            ws.open_review(Verdict::ApproveAsRevised, cx)
        })
        .expect("open_review");
    wait_for(&handle, cx, "review modal open", |ws| {
        ws.review.as_ref().map(|f| f.verdict) == Some(Verdict::ApproveAsRevised)
    })
    .await;
    // Hold the modal briefly so external capture can grab it.
    cx.background_executor()
        .timer(hold_ms("WB_HOLD_MODAL_MS", 2_500))
        .await;

    // 4. Simulate the reviewer editing the inference in the modal editor.
    handle
        .update(cx, |ws, _w, cx| {
            let flow = ws.review.as_mut().expect("review flow");
            let editor = flow.editor.clone().expect("editor");
            editor.update(cx, |ti, cx| {
                ti.content = REVISED_INFERENCE.to_string();
                ti.cursor = ti.content.len();
                cx.notify();
            });
            cx.notify();
        })
        .expect("edit inference");

    // 5. Submit (same handler as 提交审核 button): MCP write + audit + reloads.
    handle
        .update(cx, |ws, _w, cx| ws.submit_review(cx))
        .expect("submit_review");
    wait_for(&handle, cx, "review submitted (modal closed)", |ws| {
        ws.review.is_none()
    })
    .await;

    // 6. Final state: task SUCCEEDED, audit trail has the record, queue updated.
    wait_for(&handle, cx, "task refreshed to SUCCEEDED", |ws| {
        ws.detail.as_ref().map(|d| d.status.as_str()) == Some("SUCCEEDED")
    })
    .await;
    handle
        .update(cx, |ws, _w, _| {
            let d = ws.detail.as_ref().expect("detail");
            assert_eq!(d.brief_version, Some(2), "revise bumps brief version");
            assert!(
                !d.sections.inference.contains("1652"),
                "v2 inference quotes the announcement numbers"
            );
            let records = ws.audit.list();
            assert_eq!(records.len(), 1, "F7 audit trail recorded the review");
            assert_eq!(records[0].verdict, "修改后通过");
            assert!(records[0].revised);
            assert!(
                !ws.queue
                    .iter()
                    .any(|t| t.task_id == SDIC && t.status == "WAITING_REVIEW"),
                "queue no longer lists the reviewed task"
            );
            // The vault brief on disk is the clean v2.
            let brief = ws.vault.read_brief(SDIC).expect("v2 brief");
            assert_eq!(brief.frontmatter.status, "已通过");
            assert!(!brief
                .section(wb_core::vault::SectionKind::Inference)
                .unwrap()
                .body
                .contains("1652"));
        })
        .expect("final asserts");

    println!("SELFCHECK_E2E_OK: full review loop driven through the real UI state");
}
