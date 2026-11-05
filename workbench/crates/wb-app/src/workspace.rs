//! Workspace: root GPUI view wiring all WB functions —
//! review inbox (F5), brief library (F1), event quadrant (F12), audit trail (F7),
//! service status / degraded display (F10), webhook toasts (F9/I1).

use crate::i18n::{self, Lang};
use crate::input::TextInput;
use crate::theme::{sid, *};
use gpui::{div, prelude::*, px, ClickEvent, Context, Entity, Hsla, Window};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wb_core::audit::AuditLog;
use wb_core::config::AppConfig;
use wb_core::guardrail::{
    build_evidence_numbers, check as guardrail_check, EvidenceInputs, Guardrail, GuardrailConfig,
};
use wb_core::model::{
    ContextBundle, Evidence, Methodology, ReviewRequest, TaskDetail, TaskSummary, Verdict,
};
use wb_core::research::{ApprovalPolicy, ResearchClient, ResearchError};
use wb_core::state::quadrant_of;
use wb_core::vault::{Brief, Vault};
use wb_core::webhook::{WebhookEvent, WebhookListener};

gpui::actions!(
    wb_app,
    [
        RefreshQueue,
        GoBack,
        SubmitReview,
        ShowInbox,
        ShowLibrary,
        ShowQuadrant,
        ShowAudit
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Inbox,
    Library,
    Quadrant,
    AuditTrail,
}

impl ViewKind {
    pub fn label(&self) -> &'static str {
        match self {
            ViewKind::Inbox => "审核收件箱",
            ViewKind::Library => "简报库",
            ViewKind::Quadrant => "事件四象限",
            ViewKind::AuditTrail => "审核台账",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub at: Instant,
}

pub struct ReviewFlow {
    pub verdict: Verdict,
    pub comment: Entity<TextInput>,
    pub editor: Option<Entity<TextInput>>,
    pub original_inference: String,
    pub brief_version: u64,
    pub submitting: bool,
    pub error: Option<String>,
}

pub struct Workspace {
    pub cfg: AppConfig,
    pub lang: Lang,
    pub client: Arc<ResearchClient>,
    pub vault: Arc<Vault>,
    pub audit: Arc<AuditLog>,
    pub view: ViewKind,

    pub queue: Vec<TaskSummary>,
    pub queue_error: Option<String>,
    pub loading_queue: bool,
    pub status_filter: Option<String>,
    pub company_filter: String,
    pub company_input: Entity<TextInput>,
    pub search_input: Entity<TextInput>,
    /// The active library search query; an empty query means "browse", so
    /// relevance scores stay out of the rows.
    pub library_query: String,
    pub library_hits: Vec<(Brief, usize)>,
    /// Library status quick-filter (contract status, e.g. WAITING_REVIEW).
    pub library_status_filter: Option<String>,
    /// Split-view reader: the brief currently previewed from the vault.
    pub library_reader: Option<Brief>,

    pub selected_task: Option<String>,
    pub detail: Option<TaskDetail>,
    /// Full-page detail vs the right-side preview drawer. Selecting a task
    /// opens the drawer; 展开完整视图 flips to the full page.
    pub detail_expanded: bool,
    pub context: Option<ContextBundle>,
    pub evidence: Option<Evidence>,
    pub methodology: Option<Methodology>,
    pub guardrail: Option<Guardrail>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    pub show_evidence: bool,
    pub hovered_violation: Option<usize>,

    pub review: Option<ReviewFlow>,
    pub toasts: Vec<Toast>,
    /// Keyboard-shortcuts modal, toggled from the topbar ? button.
    pub show_shortcuts: bool,
    /// Collapsed system/audit metadata accordion on the detail page.
    pub show_meta: bool,
    /// Collapsed process-timeline accordion on the detail page.
    pub show_history: bool,

    pub webhook_rx: Option<Receiver<WebhookEvent>>,
    pub webhook_port: u16,
    pub service_ok: Option<bool>,
    pub last_refresh: Option<String>,
}

impl Workspace {
    pub fn new(
        initial_view: Option<ViewKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Workspace {
        let cfg = AppConfig::load();
        let lang = Lang::from_config(&cfg.lang);
        let client = Arc::new(ResearchClient::new(
            cfg.research_api_base.clone(),
            cfg.research_mcp_url.clone(),
            Duration::from_millis(cfg.timeout_ms),
            ApprovalPolicy::RequireApproval,
        ));
        let vault = Arc::new(Vault::new(cfg.vault_root.clone()));
        let audit = Arc::new(AuditLog::at(cfg.data_dir.join("reviews.jsonl")));

        // I1 webhook listener; fall back to an ephemeral port when busy.
        let (listener, rx) = match WebhookListener::start(cfg.webhook_port) {
            Ok(pair) => pair,
            Err(_) => WebhookListener::start(0).expect("ephemeral webhook listener"),
        };
        let webhook_port = listener.addr.port();

        let ts0 = i18n::strings(lang);
        const GLYPH_SEARCH: &str = "\u{E721}";
        let company_input =
            cx.new(|cx| TextInput::new(false, ts0.company_filter_placeholder, false, cx).with_icon(GLYPH_SEARCH));
        let search_input =
            cx.new(|cx| TextInput::new(false, ts0.library_search_placeholder, false, cx).with_icon(GLYPH_SEARCH));

        let mut ws = Workspace {
            cfg,
            lang,
            client,
            vault,
            audit,
            view: initial_view.unwrap_or(ViewKind::Inbox),
            queue: Vec::new(),
            queue_error: None,
            loading_queue: false,
            status_filter: None,
            company_filter: String::new(),
            company_input: company_input.clone(),
            search_input: search_input.clone(),
            library_query: String::new(),
            library_hits: Vec::new(),
            library_status_filter: None,
            library_reader: None,
            selected_task: None,
            detail: None,
            detail_expanded: false,
            context: None,
            evidence: None,
            methodology: None,
            guardrail: None,
            detail_loading: false,
            detail_error: None,
            show_evidence: false,
            hovered_violation: None,
            review: None,
            toasts: Vec::new(),
            show_shortcuts: false,
            show_meta: false,
            show_history: false,
            webhook_rx: Some(rx),
            webhook_port,
            service_ok: None,
            last_refresh: None,
        };
        ws.library_hits = ws.vault.search("");

        // Company filter drives the inbox list.
        let weak = cx.entity().downgrade();
        company_input.update(cx, |ti, cx| {
            ti.set_on_change(
                move |text, _w, app| {
                    weak.update(app, |ws, cx| {
                        ws.company_filter = text.to_string();
                        cx.notify();
                    })
                    .ok();
                },
                cx,
            );
        });
        // Library search drives the vault hits.
        let weak = cx.entity().downgrade();
        search_input.update(cx, |ti, cx| {
            ti.set_on_change(
                move |text, _w, app| {
                    weak.update(app, |ws, cx| {
                        ws.library_query = text.to_string();
                        ws.library_hits = ws.vault.search(text);
                        cx.notify();
                    })
                    .ok();
                },
                cx,
            );
        });

        ws.reload_queue(cx);
        ws.spawn_event_loop(cx);
        // Auto-focus the first input is unnecessary; ensure window focus though.
        window.refresh();
        ws
    }

    fn spawn_event_loop(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let mut had_webhook = false;
            let mut expired_toasts = false;
            let mut new_events: Vec<WebhookEvent> = Vec::new();
            this.update(cx, |ws, cx| {
                if let Some(rx) = &ws.webhook_rx {
                    while let Ok(ev) = rx.try_recv() {
                        new_events.push(ev);
                    }
                }
                let before = ws.toasts.len();
                ws.toasts
                    .retain(|t| t.at.elapsed() < Duration::from_secs(6));
                expired_toasts = ws.toasts.len() != before;
                for ev in &new_events {
                    let ts = i18n::strings(ws.lang);
                    ws.push_toast(
                        format!("{}{}", ts.toast_brief_ready, ev.title),
                        ToastKind::Info,
                        cx,
                    );
                }
                had_webhook = !new_events.is_empty();
            })
            .ok();
            if had_webhook {
                this.update(cx, |ws, cx| ws.reload_queue(cx)).ok();
            } else if expired_toasts {
                this.update(cx, |_ws, cx| cx.notify()).ok();
            }
        })
        .detach();
    }

    pub fn push_toast(&mut self, text: String, kind: ToastKind, cx: &mut Context<Self>) {
        self.toasts.push(Toast {
            text,
            kind,
            at: Instant::now(),
        });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
        cx.notify();
    }

    // ------------------------------------------------------------- data flow

    pub fn toggle_language(&mut self, cx: &mut Context<Self>) {
        self.lang = self.lang.toggle();
        self.cfg.lang = self.lang.to_config().to_string();
        self.cfg.save();
        let ts = i18n::strings(self.lang);
        self.company_input.update(cx, |ti, cx| {
            ti.placeholder = ts.company_filter_placeholder.into();
            cx.notify();
        });
        self.search_input.update(cx, |ti, cx| {
            ti.placeholder = ts.library_search_placeholder.into();
            cx.notify();
        });
        cx.notify();
    }

    pub fn reload_queue(&mut self, cx: &mut Context<Self>) {
        self.loading_queue = true;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { client.list_pending(None) })
                .await;
            this.update(cx, |ws, cx| {
                ws.loading_queue = false;
                match res {
                    Ok(tasks) => {
                        ws.queue = tasks;
                        ws.queue_error = None;
                        ws.service_ok = Some(true);
                    }
                    Err(e) => {
                        ws.queue_error = Some(e.to_string());
                        ws.service_ok = Some(false);
                    }
                }
                ws.last_refresh = Some(chrono::Local::now().format("%H:%M:%S").to_string());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        self.selected_task = Some(task_id.to_string());
        self.detail_loading = true;
        self.detail = None;
        self.detail_error = None;
        self.show_evidence = false;
        self.detail_expanded = false;
        self.hovered_violation = None;
        cx.notify();

        let client = self.client.clone();
        let vault = self.vault.clone();
        let tid = task_id.to_string();
        let tid2 = tid.clone();
        cx.spawn(async move |this, cx| {
            let bundle = cx
                .background_executor()
                .spawn(async move {
                    let detail = client.get_task(&tid2);
                    let context = client.get_context(&tid2);
                    let evidence = client.get_evidence(&tid2);
                    let methodology = client.get_methodology(&tid2);
                    (detail, context, evidence, methodology)
                })
                .await;
            this.update(cx, |ws, cx| {
                // A newer open_task may have superseded this load; never let a
                // stale response overwrite the currently selected task.
                if ws.selected_task.as_deref() != Some(tid.as_str()) {
                    return;
                }
                let (detail, context, evidence, methodology) = bundle;
                match detail {
                    Ok(mut d) => {
                        // Guardrail input: deterministic metrics + free-text evidence
                        // + the vault brief's CALCULATION section.
                        let mut inputs = EvidenceInputs::default();
                        if let Ok(ctx) = &context {
                            inputs.metrics =
                                ctx.metrics.0.values().filter_map(|v| v.as_f64()).collect();
                            inputs.free_texts = vec![ctx.announcement.summary.clone()];
                        }
                        if let Ok(ev) = &evidence {
                            inputs.free_texts.push(ev.excerpt.clone());
                        }
                        if let Some(brief) = vault.read_brief(&tid) {
                            inputs.metrics.extend(brief.calc_values().values().copied());
                        }
                        // Calibrate with the server's own tolerances when available.
                        let guard_cfg = methodology
                            .as_ref()
                            .ok()
                            .map(|m| GuardrailConfig {
                                tolerance_pct: m.validation.tolerance_pct,
                                pp_tolerance: m.validation.pp_tolerance,
                            })
                            .unwrap_or_default();
                        ws.guardrail = Some(guardrail_check(
                            &d.sections.inference,
                            &build_evidence_numbers(&inputs),
                            &guard_cfg,
                        ));
                        if let Ok(ctx) = context {
                            ws.context = Some(ctx);
                        }
                        if let Ok(ev) = evidence {
                            ws.evidence = Some(ev);
                        }
                        if let Ok(m) = methodology {
                            ws.methodology = Some(m);
                        }
                        d.brief_version.get_or_insert(1);
                        ws.detail = Some(d);
                        ws.detail_error = None;
                    }
                    Err(e) => {
                        // Degraded fallback: build the detail from the vault brief (I2).
                        match ws.detail_from_vault(&tid) {
                            Some(mut d) => {
                                let mut inputs = EvidenceInputs::default();
                                if let Some(brief) = vault.read_brief(&tid) {
                                    inputs.metrics.extend(brief.calc_values().values().copied());
                                    if let Some(fact) =
                                        brief.section(wb_core::vault::SectionKind::Fact)
                                    {
                                        inputs.free_texts.push(fact.body.clone());
                                    }
                                }
                                ws.guardrail = Some(guardrail_check(
                                    &d.sections.inference,
                                    &build_evidence_numbers(&inputs),
                                    &Default::default(),
                                ));
                                d.brief_version.get_or_insert(1);
                                ws.detail = Some(d);
                                let ts = i18n::strings(ws.lang);
                                ws.detail_error =
                                    Some(format!("{}{e}）", ts.toast_degraded_render));
                            }
                            None => {
                                ws.detail = None;
                                ws.detail_error = Some(e.to_string());
                            }
                        }
                    }
                }
                ws.detail_loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Split-view reader: preview a vault brief beside the library list.
    pub fn open_library_reader(&mut self, brief: Brief, cx: &mut Context<Self>) {
        self.library_reader = Some(brief);
        cx.notify();
    }

    fn detail_from_vault(&self, task_id: &str) -> Option<TaskDetail> {
        let brief = self.vault.read_brief(task_id)?;
        let fm = &brief.frontmatter;
        Some(TaskDetail {
            task_id: fm.task_id.clone(),
            trace_id: fm.trace_id.clone(),
            object_id: fm.object_id.clone(),
            object_name: brief
                .title
                .split(' ')
                .next()
                .unwrap_or(&fm.object_id)
                .to_string(),
            period: fm.period.clone(),
            status: fm.status.clone(),
            workflow_version: fm.workflow_version.clone(),
            data_cutoff: fm.data_cutoff.clone(),
            source_version: fm.source_version.clone(),
            created_at: fm.created_at.clone(),
            updated_at: fm.created_at.clone(),
            sections: wb_core::model::BriefSections {
                fact: wb_core::model::FactList(
                    brief
                        .section(wb_core::vault::SectionKind::Fact)
                        .map(|s| {
                            s.body
                                .lines()
                                .map(|l| l.trim_start_matches("- ").to_string())
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                calculation: wb_core::model::CalcValues(
                    brief
                        .calc_values()
                        .into_iter()
                        .map(|(k, v)| (k, serde_json::json!(v)))
                        .collect(),
                ),
                inference: brief
                    .section(wb_core::vault::SectionKind::Inference)
                    .map(|s| s.body.clone())
                    .unwrap_or_default(),
                check: brief
                    .section(wb_core::vault::SectionKind::Check)
                    .map(|s| s.body.clone())
                    .unwrap_or_default(),
            },
            history: Vec::new(),
            brief_path: None,
            brief_version: Some(1),
            fail_reason: None,
        })
    }

    pub fn open_review(&mut self, verdict: Verdict, cx: &mut Context<Self>) {
        let Some(detail) = &self.detail else { return };
        let version = detail.brief_version.unwrap_or(1);
        let comment = cx.new(|cx| {
            TextInput::new(
                false,
                i18n::strings(self.lang).comment_placeholder,
                false,
                cx,
            )
        });
        // Always create the revise editor: the modal's verdict tabs let the
        // reviewer switch to 修改后通过 after opening with 通过/驳回, and the
        // editor must be there when they do (pre-filled with the original draft).
        let editor = cx.new(|cx| {
            TextInput::new(true, i18n::strings(self.lang).revise_placeholder, true, cx)
                .with_content(&detail.sections.inference)
        });
        let flow = ReviewFlow {
            verdict,
            comment,
            editor: Some(editor),
            original_inference: detail.sections.inference.clone(),
            brief_version: version,
            submitting: false,
            error: None,
        };
        if let Some(editor) = &flow.editor {
            let weak = cx.entity().downgrade();
            editor.update(cx, |ti, cx| {
                ti.set_on_change(
                    move |_text, _w, app| {
                        weak.update(app, |ws, cx| {
                            if let Some(flow) = &mut ws.review {
                                flow.error = None;
                            }
                            cx.notify();
                        })
                        .ok();
                    },
                    cx,
                );
            });
        }
        self.review = Some(flow);
        cx.notify();
    }

    pub fn submit_review(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = &mut self.review else { return };
        // Ignore re-entrant submits (Ctrl+Enter, double click) while a request
        // is already in flight — the idempotency key would be identical, but
        // the audit trail must record the review exactly once.
        if flow.submitting {
            return;
        }
        let Some(detail) = &self.detail else { return };
        let verdict = flow.verdict;
        let comment = flow.comment.read(cx).text().to_string();
        // Only 修改后通过 carries revised sections; Approve/Reject must not send
        // the editor's (unchanged) content as if it were a revision.
        let revised = (verdict == Verdict::ApproveAsRevised)
            .then(|| flow.editor.as_ref().map(|e| e.read(cx).text().to_string()))
            .flatten();
        // Client-side guard: an empty 修改稿 would only earn a server 400.
        if verdict == Verdict::ApproveAsRevised
            && revised
                .as_deref()
                .map(|t| t.trim().is_empty())
                .unwrap_or(true)
        {
            if let Some(flow) = &mut self.review {
                flow.error = Some(i18n::strings(self.lang).revise_empty.into());
            }
            cx.notify();
            return;
        }
        let version = flow.brief_version;
        let task_id = detail.task_id.clone();
        let reviewer = self.cfg.reviewer.clone();
        let idempotency_key = review_idempotency_key(&task_id, version);
        let client = self.client.clone();

        // Optimistically mark submitting; move the request to the background.
        if let Some(flow) = &mut self.review {
            flow.submitting = true;
            flow.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let req = ReviewRequest {
                task_id: task_id.clone(),
                verdict,
                comment: if comment.is_empty() {
                    None
                } else {
                    Some(comment)
                },
                reviewer: reviewer.clone(),
                idempotency_key: idempotency_key.clone(),
                revised_sections: revised.map(|inference| {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert("inference".to_string(), inference);
                    m
                }),
            };
            let req_for_audit = req.clone();
            let res = cx
                .background_executor()
                .spawn(async move { client.submit_review(&req, true) })
                .await;
            this.update(cx, |ws, cx| match res {
                Ok(receipt) => {
                    let ts = i18n::strings(ws.lang);
                    let object_name = ws
                        .detail
                        .as_ref()
                        .map(|d| d.object_name.clone())
                        .unwrap_or_default();
                    let period = ws
                        .detail
                        .as_ref()
                        .map(|d| d.period.clone())
                        .unwrap_or_default();
                    if let Err(e) =
                        ws.audit
                            .record_review(&req_for_audit, &object_name, &period, &receipt)
                    {
                        ws.push_toast(
                            format!("{}{e}", ts.toast_audit_failed),
                            ToastKind::Error,
                            cx,
                        );
                    }
                    ws.review = None;
                    ws.hovered_violation = None;
                    let verdict_label = i18n::verdict_label(ws.lang, verdict.as_str());
                    let msg = match (ws.lang, receipt.brief_version) {
                        (Lang::Zh, Some(v)) => format!(
                            "{}{} → {}（简报 v{v}，{}）",
                            ts.toast_review_done, verdict_label, receipt.status, receipt.review_id
                        ),
                        (Lang::En, Some(v)) => format!(
                            "{}{} → {} (brief v{v}, {})",
                            ts.toast_review_done, verdict_label, receipt.status, receipt.review_id
                        ),
                        (_, _) => format!(
                            "{}{} → {}",
                            ts.toast_review_done, verdict_label, receipt.review_id
                        ),
                    };
                    ws.push_toast(msg, ToastKind::Success, cx);
                    ws.reload_queue(cx);
                    if ws.selected_task.as_deref() == Some(task_id.as_str()) {
                        ws.open_task(&task_id, cx);
                    }
                }
                Err(ResearchError::Conflict {
                    message,
                    current_status,
                }) => {
                    if let Some(flow) = &mut ws.review {
                        flow.submitting = false;
                        flow.error = Some(match ws.lang {
                            Lang::Zh => format!("状态冲突 409：{message}（当前 {current_status}）"),
                            Lang::En => {
                                format!("409 conflict: {message} (current {current_status})")
                            }
                        });
                    }
                    let ts = i18n::strings(ws.lang);
                    ws.push_toast(
                        format!("{}{current_status})", ts.toast_review_conflict),
                        ToastKind::Warn,
                        cx,
                    );
                    ws.reload_queue(cx);
                    if ws.selected_task.as_deref() == Some(task_id.as_str()) {
                        ws.open_task(&task_id, cx);
                    }
                }
                Err(e) => {
                    if let Some(flow) = &mut ws.review {
                        flow.submitting = false;
                        flow.error = Some(match ws.lang {
                            Lang::Zh => format!("提交失败（{}）：{e}", e.kind()),
                            Lang::En => format!("Submit failed ({}): {e}", e.kind()),
                        });
                    }
                    let ts = i18n::strings(ws.lang);
                    ws.push_toast(
                        format!("{}{e}", ts.toast_review_failed),
                        ToastKind::Error,
                        cx,
                    );
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn export_audit(&mut self, cx: &mut Context<Self>) {
        let dir = self
            .audit
            .path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let rx = cx.prompt_for_new_path(&dir, Some("wb-reviews-export.json"));
        let audit = self.audit.clone();
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            this.update(cx, |ws, cx| {
                let ts = i18n::strings(ws.lang);
                match picked {
                    Some(path) => match audit.export_json(&path) {
                        Ok(n) => ws.push_toast(
                            ts.toast_exported
                                .replacen("{}", &n.to_string(), 1)
                                .replacen("{}", &path.display().to_string(), 1),
                            ToastKind::Success,
                            cx,
                        ),
                        Err(e) => ws.push_toast(
                            format!("{}{e}", ts.toast_review_failed),
                            ToastKind::Error,
                            cx,
                        ),
                    },
                    None => {
                        ws.push_toast(ts.toast_export_cancelled.to_string(), ToastKind::Info, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }
}

// ============================================================ rendering (v3)
// Design rules: 4px grid, one accent, dot-based status, hairline structure,
// transparent list rows with hover lift, quiet mono meta.

impl Workspace {
    pub fn render_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Width of the content area right of the sidebar, in logical pixels.
        let pane_w = (window.viewport_size().width / window.scale_factor()) - px(232.0);
        let main_pane: gpui::AnyElement = if self.show_shortcuts {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(self.render_shortcuts_modal(window, cx))
                .into_any_element()
        } else {
            match self.view {
                ViewKind::Inbox | ViewKind::Quadrant => {
                    // Both views share the task pane: review modal, full
                    // detail, or the preview drawer beside the list/grid.
                    if self.review.is_some() {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .p_5()
                            .id("modal-scroll")
                            .overflow_y_scroll()
                            .child(self.render_review_modal(window, cx))
                            .into_any_element()
                    } else if self.selected_task.is_some() && self.detail_expanded {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .px_5()
                            .py_4()
                            .id("detail-scroll")
                            .overflow_y_scroll()
                            .child(self.render_detail(window, cx))
                            .into_any_element()
                    } else if self.selected_task.is_some() {
                        // Master-detail: a fixed-width list/grid and a drawer
                        // that fills every remaining pixel (fixed first, grow
                        // last — the one flex arrangement gpui 0.2.2 on
                        // Windows gets right). The list yields when the pane
                        // is narrow, or the drawer would overflow the window.
                        let list_w = if self.view == ViewKind::Quadrant {
                            px(640.0).min(pane_w - px(460.0)).max(px(360.0))
                        } else {
                            px(560.0).min(pane_w - px(420.0)).max(px(320.0))
                        };
                        let left = match self.view {
                            ViewKind::Quadrant => div()
                                .flex()
                                .flex_col()
                                .w(list_w)
                                .min_w(list_w)
                                .min_h_0()
                                .overflow_hidden()
                                .child(self.render_quadrant(window, cx, list_w)),
                            _ => div()
                                .flex()
                                .flex_col()
                                .w(list_w)
                                .min_w(list_w)
                                .min_h_0()
                                .overflow_hidden()
                                .child(self.render_inbox(false, cx)),
                        };
                        div()
                            .flex()
                            .flex_row()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(left)
                            .child(self.render_detail_panel(window, cx))
                            .into_any_element()
                    } else {
                        let avail = (window.viewport_size().width / window.scale_factor())
                            - px(232.0);
                        let pane: gpui::AnyElement = match self.view {
                            ViewKind::Quadrant => div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h_0()
                                .child(self.render_quadrant(window, cx, avail))
                                .into_any_element(),
                            _ => div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h_0()
                                .child(self.render_inbox(true, cx))
                                .into_any_element(),
                        };
                        pane
                    }
                }
                ViewKind::Library => {
                    if self.library_reader.is_some() {
                        // Split-view reader: fixed-width list, the brief's
                        // markdown fills the rest. The list yields when the
                        // pane is narrow.
                        let list_w = px(560.0).min(pane_w - px(420.0)).max(px(320.0));
                        div()
                            .flex()
                            .flex_row()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w(list_w)
                                    .min_w(list_w)
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(self.render_library(cx)),
                            )
                            .child(self.render_library_reader(window, cx))
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_library(cx))
                            .into_any_element()
                    }
                }
                ViewKind::AuditTrail => div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_audit(window, cx))
                    .into_any_element(),
            }
        };

        div()
            .id("root")
            .size_full()
            .flex()
            .flex_col()
            .bg(c(BG))
            .text_color(c(FG))
            .font_family(FONT_UI)
            .on_action(cx.listener(|ws, _: &RefreshQueue, _w, cx| ws.reload_queue(cx)))
            .on_action(cx.listener(|ws, _: &GoBack, _w, cx| {
                if ws.show_shortcuts {
                    ws.show_shortcuts = false;
                } else if ws.review.is_some() {
                    ws.review = None;
                    ws.hovered_violation = None;
                } else if ws.view == ViewKind::Library && ws.library_reader.is_some() {
                    ws.library_reader = None;
                } else if ws.selected_task.is_some() {
                    if ws.detail_expanded {
                        // Back from the full page to the preview drawer.
                        ws.detail_expanded = false;
                    } else {
                        ws.selected_task = None;
                        ws.detail = None;
                        ws.guardrail = None;
                        ws.show_evidence = false;
                    }
                }
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &SubmitReview, _w, cx| {
                if ws.review.is_some() {
                    ws.submit_review(cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &ShowInbox, _w, cx| {
                ws.view = ViewKind::Inbox;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ShowLibrary, _w, cx| {
                ws.view = ViewKind::Library;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ShowQuadrant, _w, cx| {
                ws.view = ViewKind::Quadrant;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ShowAudit, _w, cx| {
                ws.view = ViewKind::AuditTrail;
                cx.notify();
            }))
            .child(self.render_topbar(window, cx))
            .when(!self.toasts.is_empty(), |el| {
                el.child(self.render_toasts(window))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(main_pane),
            )
    }

    /// 44px top bar: identity left, connection + language + refresh right.
    fn render_topbar(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let (dot, label) = match self.service_ok {
            Some(true) => (c(GREEN), ts.service_ok.to_string()),
            Some(false) => (
                c(RED),
                format!(
                    "{}{}",
                    ts.service_down,
                    self.queue_error.as_deref().unwrap_or("")
                ),
            ),
            None => (c(FG3), ts.connecting.to_string()),
        };
        // Surface the last successful refresh next to the connection state (F10).
        let label = match &self.last_refresh {
            Some(at) => format!("{label}{}{at}", ts.refreshed_at),
            None => label,
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .h(px(44.0))
            .px_4()
            .bg(c(SURFACE))
            .border_b_1()
            .border_color(c(HAIRLINE))
            .child(
                div()
                    .text_size(px(T13))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(c(FG))
                    .child(ts.app_title),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(T12))
                    .text_color(if matches!(self.service_ok, Some(true)) {
                        c(FG2)
                    } else {
                        c(RED)
                    })
                    .child(div().w(px(7.0)).h(px(7.0)).rounded_full().bg(dot))
                    .child(div().max_w(px(360.0)).truncate().child(label)),
            )
            .child(self.button(
                self.lang.toggle_label(),
                Btn::Ghost,
                None,
                _w,
                cx,
                |ws, _e, _w, cx| {
                    ws.toggle_language(cx);
                },
            ))
            .child(self.button(
                ts.refresh_queue,
                Btn::Ghost,
                Some(GLYPH_REFRESH),
                _w,
                cx,
                |ws, _e, _w, cx| {
                    ws.reload_queue(cx);
                },
            ))
            .child(div().flex_1())
            .child(self.button(
                "?",
                Btn::Ghost,
                None,
                _w,
                cx,
                |ws, _e, _w, cx| {
                    ws.show_shortcuts = !ws.show_shortcuts;
                    cx.notify();
                },
            ))
    }

    /// 220px nav column on SURFACE with a quiet stats/config footer.
    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let waiting = self
            .queue
            .iter()
            .filter(|t| t.status == "WAITING_REVIEW")
            .count();
        let failed = self.queue.iter().filter(|t| t.status == "FAILED").count();
        let items = [
            (ViewKind::Inbox, GLYPH_INBOX, ts.nav_inbox),
            (ViewKind::Library, GLYPH_LIB, ts.nav_library),
            (ViewKind::Quadrant, GLYPH_QUAD, ts.nav_quadrant),
            (ViewKind::AuditTrail, GLYPH_HIST, ts.nav_audit),
        ];
        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(232.0))
            .min_w(px(232.0))
            .h_full()
            .bg(c(SURFACE))
            .border_r_1()
            .border_color(c(HAIRLINE))
            .p_2()
            .gap_0p5();
        for (kind, glyph, label) in items {
            let active = self.view == kind;
            let mut item = div()
                .id(sid(format!("nav-{}", kind.label())))
                .flex()
                .flex_row()
                .items_center()
                .gap_2p5()
                .h(px(34.0))
                .px_2p5()
                .rounded_md();
            item = if active {
                item.bg(c(RAISED))
            } else {
                item.hover(|s| s.bg(c(RAISED)))
            };
            sidebar = sidebar.child(
                item.child(icon(glyph, 15.0, if active { c(ACCENT) } else { c(FG3) }))
                    .child(
                        div()
                            .text_size(px(T13))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(if active { c(FG) } else { c(FG2) })
                            .child(label),
                    )
                    .child(div().flex_1())
                    .when(kind == ViewKind::Inbox && waiting > 0, |el| {
                        el.child(
                            div()
                                .text_size(px(T11))
                                .font_family(FONT_MONO)
                                .text_color(c(AMBER))
                                .child(format!("{waiting}")),
                        )
                    })
                    .on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                        ws.view = kind;
                        ws.hovered_violation = None;
                        cx.notify();
                    })),
            );
        }
        // Footer: quiet stats + compact config + shortcuts.
        sidebar = sidebar.child(
            div()
                .mt_3()
                .pt_3()
                .border_t_1()
                .border_color(c(HAIRLINE))
                .px_2p5()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_size(px(T11))
                        .text_color(c(FG3))
                        .child(match self.lang {
                            Lang::Zh => format!(
                                "待审核 {waiting} · 失败 {failed} · 简报 {}",
                                self.library_hits.len()
                            ),
                            Lang::En => format!(
                                "{waiting} pending · {failed} failed · {} briefs",
                                self.library_hits.len()
                            ),
                        }),
                )
                .child(
                    div()
                        .text_size(px(T11))
                        .font_family(FONT_MONO)
                        .text_color(c(FG3))
                        .child(format!(
                            "api :{} · I1 :{}",
                            api_port(&self.cfg.research_api_base),
                            self.webhook_port
                        )),
                ),
        );
        sidebar
    }

    /// Toasts: quiet strip under the topbar — dot + text, no color blocks.
    fn render_toasts(&mut self, _w: &mut Window) -> gpui::Div {
        let mut strip = div()
            .flex()
            .flex_col()
            .px_4()
            .py_2()
            .gap_1p5()
            .bg(c(SURFACE))
            .border_b_1()
            .border_color(c(HAIRLINE));
        for t in &self.toasts {
            let dot = match t.kind {
                ToastKind::Info => c(ACCENT),
                ToastKind::Success => c(GREEN),
                ToastKind::Warn => c(AMBER),
                ToastKind::Error => c(RED),
            };
            strip = strip.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(dot))
                    .child(
                        div()
                            .text_size(px(T12))
                            .text_color(c(FG))
                            .child(t.text.clone()),
                    ),
            );
        }
        strip
    }

    /// Keyboard shortcuts modal: keycap + plain-language description per row.
    /// Opens from the topbar ? button; Esc or 关闭 dismisses it.
    fn render_shortcuts_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let rows: [(&str, &str); 7] = [
            ("Ctrl+1", ts.sc_inbox),
            ("Ctrl+2", ts.sc_library),
            ("Ctrl+3", ts.sc_quadrant),
            ("Ctrl+4", ts.sc_audit),
            ("Ctrl+R", ts.sc_refresh),
            ("Esc", ts.sc_back),
            ("Ctrl+Enter", ts.sc_submit),
        ];
        let mut list = div().flex().flex_col();
        for (key, desc) in rows {
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(32.0))
                    .child(keycap(key))
                    .child(
                        div()
                            .text_size(px(T13))
                            .text_color(c(FG2))
                            .child(desc.to_string()),
                    ),
            );
        }
        let avail = (window.viewport_size().width / window.scale_factor()) - px(232.0 + 16.0 * 2.0);
        let modal_ml = ((avail - px(440.0)) / 2.0).max(px(0.0));
        div().flex().flex_col().flex_1().min_h_0().justify_center().child(
            div()
                .ml(modal_ml)
                .w(px(440.0))
                .flex()
                .flex_col()
                .gap_2()
                .rounded_lg()
                .bg(c(PANEL))
                .border_1()
                .border_color(c(BORDER))
                .p_4()
                .child(
                    div()
                        .text_size(px(T15))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(c(FG))
                        .child(ts.shortcuts_title),
                )
                .child(list)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .pt_1()
                        .child(self.button(ts.close, Btn::Secondary, None, window, cx, |ws, _e, _w, cx| {
                            ws.show_shortcuts = false;
                            cx.notify();
                        })),
                ),
        )
    }

    /// Button styles: Primary (accent fill), Secondary (raised), Ghost (transparent),
    /// DangerGhost (red text). Icon renders in the icon font — never inline in text.
    pub fn button(
        &mut self,
        label: &str,
        style: Btn,
        icon_glyph: Option<&'static str>,
        _w: &mut Window,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Workspace, &ClickEvent, &mut Window, &mut Context<Workspace>) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::FontWeight;
        let listener = cx.listener(handler);
        let (bg, fg, hover_bg): (Hsla, Hsla, Hsla) = match style {
            Btn::Primary => (c(ACCENT), c(0x10131a), c(0x8db9ff)),
            Btn::Success => (c(GREEN), c(0x10131a), c(0x82d9a6)),
            Btn::Secondary => (c(RAISED), c(FG), c(RAISED_HOVER)),
            Btn::Ghost => (c(SURFACE), c(FG2), c(RAISED)),
            Btn::DangerGhost => (c(SURFACE), c(RED), c(RED_TINT)),
        };
        let mut el = div()
            .id(sid(format!("btn-{}", sid(label.to_string()))))
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .px_2p5()
            .h(px(28.0))
            .rounded_md()
            .text_size(px(T12))
            .font_weight(if style == Btn::Primary {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .bg(bg)
            .text_color(fg)
            .hover(move |s| s.bg(hover_bg));
        // Reject reads as an outlined destructive action, distinct from the
        // two passing decisions.
        if style == Btn::DangerGhost {
            el = el.border_1().border_color(c(RED));
        }
        if let Some(glyph) = icon_glyph {
            let icon_color = match style {
                Btn::Primary | Btn::Success => c(0x10131a),
                Btn::DangerGhost => c(RED),
                _ => c(FG2),
            };
            el = el.child(icon(glyph, 13.0, icon_color));
        }
        el.child(label.to_string())
            .on_click(move |e, w, cx| listener(e, w, cx))
    }

    /// Review inbox: header, segmented filter, aligned list rows.
    fn render_inbox(&mut self, with_ids: bool, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let chips: [Option<&str>; 5] = [
            None,
            Some("WAITING_REVIEW"),
            Some("SUCCEEDED"),
            Some("FAILED"),
            Some("CANCELLED"),
        ];

        // min_w_0 lets the pane shrink next to the preview drawer; without it
        // the rows' fixed columns force the drawer off-screen.
        let mut pane = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        pane = pane.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_5()
                .pt_4()
                .pb_3()
                .child(
                    div()
                        .text_size(px(T15))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(c(FG))
                        .child(ts.inbox_title),
                )
                .child(
                    div()
                        .text_size(px(T12))
                        .text_color(c(FG3))
                        .child(ts.inbox_hint),
                ),
        );

        // Filter row: segmented control + company search.
        let mut filter_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_5()
            .pb_3();
        for chip in chips {
            let active = self.status_filter.as_deref() == chip;
            let count = self
                .queue
                .iter()
                .filter(|t| chip.map(|cs| t.status == cs).unwrap_or(true))
                .count();
            let label = match chip {
                None => format!("{} {}", ts.all, count),
                Some(cs) => format!("{} {}", i18n::status_label(self.lang, cs), count),
            };
            let mut el = div()
                .id(sid(format!("chip-{}", chip.unwrap_or("all"))))
                .flex()
                .flex_row()
                .items_center()
                .h(px(24.0))
                .px_2p5()
                .rounded_md()
                .text_size(px(T12));
            el = if active {
                el.bg(c(ACCENT_DIM))
                    .text_color(c(ACCENT))
                    .font_weight(gpui::FontWeight::MEDIUM)
            } else {
                el.text_color(c(FG2))
                    .hover(|s| s.bg(c(RAISED)).text_color(c(FG)))
            };
            filter_row = filter_row.child(el.child(label).on_click(cx.listener(
                move |ws, _e: &ClickEvent, _w, cx| {
                    ws.status_filter = chip.map(String::from);
                    cx.notify();
                },
            )));
        }
        filter_row = filter_row
            .child(div().w(px(260.0)).ml_3().child(self.company_input.clone()))
            .child(div().flex_1());
        pane = pane.child(filter_row);

        // Degraded notice (F10).
        if let Some(err) = &self.queue_error {
            pane = pane.child(
                div()
                    .mx_5()
                    .mb_2()
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .bg(c(AMBER_TINT))
                    .text_size(px(T12))
                    .text_color(c(AMBER))
                    .child(format!("{}{err}", ts.queue_degraded)),
            );
        }
        if self.loading_queue {
            pane = pane.child(
                div()
                    .px_5()
                    .py_4()
                    .text_size(px(T12))
                    .text_color(c(FG3))
                    .child(ts.loading_queue),
            );
        }

        // Column headers keep the table scannable as the queue grows; widths
        // mirror the row columns below.
        pane = pane.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .px_3()
                .mx_2()
                .h(px(28.0))
                .border_b_1()
                .border_color(c(HAIRLINE))
                .text_size(px(T11))
                .text_color(c(FG3))
                .child(div().w(px(56.0)).child(ts.col_priority))
                .child(div().w(px(96.0)).child(ts.col_company))
                .child(div().w(px(56.0)).child(ts.col_period))
                .child(div().w(px(104.0)).child(ts.col_status))
                .child(div().w(px(84.0)).child(ts.col_time))
                .when(with_ids, |el| el.child(div().flex_1().min_w_0().child(ts.col_task))),
        );

        // List: transparent rows + hairline dividers + hover lift.
        let filter = self.status_filter.clone();
        let company = self.company_filter.to_lowercase();
        let rows: Vec<&TaskSummary> = self
            .queue
            .iter()
            .filter(|t| filter.as_deref().map(|f| t.status == f).unwrap_or(true))
            .filter(|t| {
                company.is_empty()
                    || t.object_name.to_lowercase().contains(&company)
                    || t.object_id.to_lowercase().contains(&company)
                    || t.task_id.to_lowercase().contains(&company)
            })
            .collect();

        let mut list = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px_2()
            .id("inbox-list")
            .overflow_y_scroll();
        if rows.is_empty() && !self.loading_queue {
            list =
                list.child(div().p_8().flex().flex_col().items_center().child(
                    div().text_size(px(T13)).text_color(c(FG2)).child(
                        if self.queue_error.is_some() {
                            ts.queue_unavailable.to_string()
                        } else {
                            ts.queue_empty.to_string()
                        },
                    ),
                ));
        }
        let n = rows.len();
        for (idx, t) in rows.iter().enumerate() {
            let tid = t.task_id.clone();
            let is_last = idx + 1 == n;
            let mut row = div()
                .id(sid(format!("row-{tid}")))
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .px_3()
                .h(px(44.0))
                .rounded_md();
            row = if is_last {
                row.hover(|s| s.bg(c(RAISED_HOVER)))
            } else {
                row.border_b_1()
                    .border_color(c(HAIRLINE))
                    .hover(|s| s.bg(c(RAISED_HOVER)).border_color(c(HAIRLINE)))
            };
            let row = row
                .child(
                    div()
                        .w(px(56.0))
                        .text_size(px(T11))
                        .font_family(FONT_MONO)
                        .text_color(priority_color(&t.priority))
                        .child(t.priority.clone()),
                )
                .child(
                    div()
                        .w(px(96.0))
                        .text_size(px(T13))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(c(FG))
                        .truncate()
                        .child(t.object_name.clone()),
                )
                .child(
                    div()
                        .w(px(56.0))
                        .text_size(px(T11))
                        .font_family(FONT_MONO)
                        .text_color(c(FG2))
                        .child(t.period.clone()),
                )
                .child(
                    div().w(px(104.0)).child(Badge::new(
                        i18n::status_label(self.lang, &t.status),
                        status_tone(&t.status),
                    )),
                )
                .child(
                    div()
                        .w(px(84.0))
                        .text_size(px(T11))
                        .font_family(FONT_MONO)
                        .text_color(c(FG2))
                        .child(human_time(self.lang, &t.updated_at)),
                )
                .when(with_ids, |el| {
                    el.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(T11))
                            .font_family(FONT_MONO)
                            .text_color(c(FG2))
                            .truncate()
                            .child(tid.clone()),
                    )
                });
            list = list.child(
                row.on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                    ws.open_task(&tid, cx);
                })),
            );
        }
        pane = pane.child(list);
        pane
    }

    /// Brief library (F1): transparent rows, title + meta.
    fn render_library(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut pane = div().flex().flex_col().flex_1().min_h_0();
        pane = pane.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_5()
                .pt_4()
                .pb_3()
                .child(
                    div()
                        .text_size(px(T15))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(c(FG))
                        .child(ts.library_title),
                )
                .child(
                    div()
                        .text_size(px(T12))
                        .text_color(c(FG2))
                        .child(format!("vault：{}", self.cfg.vault_root.display())),
                ),
        );
        pane = pane.child(
            div()
                .px_5()
                .pb_3()
                .w(px(420.0))
                .child(self.search_input.clone()),
        );

        // Status quick-filters — the same segmented control as the inbox.
        let chips: [Option<&str>; 5] = [
            None,
            Some("WAITING_REVIEW"),
            Some("SUCCEEDED"),
            Some("FAILED"),
            Some("CANCELLED"),
        ];
        let mut chip_row = div().flex().flex_row().flex_wrap().items_center().gap_1().px_5().pb_3();
        for chip in chips {
            let active = self.library_status_filter.as_deref() == chip;
            let count = self
                .library_hits
                .iter()
                .filter(|(b, _)| chip.map(|c| status_matches(c, &b.frontmatter.status)).unwrap_or(true))
                .count();
            let label = match chip {
                None => format!("{} {}", ts.all, count),
                Some(cs) => format!("{} {}", i18n::status_label(self.lang, cs), count),
            };
            let mut el = div()
                .id(sid(format!("lib-chip-{}", chip.unwrap_or("all"))))
                .flex()
                .flex_row()
                .items_center()
                .h(px(24.0))
                .px_2p5()
                .rounded_md()
                .text_size(px(T12));
            el = if active {
                el.bg(c(ACCENT_DIM))
                    .text_color(c(ACCENT))
                    .font_weight(gpui::FontWeight::MEDIUM)
            } else {
                el.text_color(c(FG2))
                    .hover(|s| s.bg(c(RAISED)).text_color(c(FG)))
            };
            chip_row = chip_row.child(el.child(label).on_click(cx.listener(
                move |ws, _e: &ClickEvent, _w, cx| {
                    ws.library_status_filter = chip.map(String::from);
                    cx.notify();
                },
            )));
        }
        pane = pane.child(chip_row);

        let mut list = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px_2()
            .id("library-list")
            .overflow_y_scroll();
        let rows: Vec<&(Brief, usize)> = self
            .library_hits
            .iter()
            .filter(|(b, _)| {
                self.library_status_filter
                    .as_deref()
                    .map(|f| status_matches(f, &b.frontmatter.status))
                    .unwrap_or(true)
            })
            .collect();
        if rows.is_empty() {
            list = list.child(
                div()
                    .p_8()
                    .text_size(px(T13))
                    .text_color(c(FG2))
                    .child(ts.library_empty),
            );
        }
        let n = rows.len();
        for (idx, (brief, score)) in rows.into_iter().enumerate() {
            let fm = &brief.frontmatter;
            let tid = fm.task_id.clone();
            let status = fm.status.clone();
            let is_last = idx + 1 == n;
            let mut row = div()
                .id(sid(format!("brief-{tid}")))
                .flex()
                .flex_col()
                .gap_0p5()
                .px_3()
                .py_2()
                .rounded_md();
            row = if is_last {
                row.hover(|s| s.bg(c(RAISED_HOVER)))
            } else {
                row.border_b_1()
                    .border_color(c(HAIRLINE))
                    .hover(|s| s.bg(c(RAISED_HOVER)).border_color(c(HAIRLINE)))
            };
            let _ = score;
            let row = row
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2p5()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(T13))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(c(FG))
                                .truncate()
                                .child(brief.title.clone()),
                        )
                        .child(Badge::new(
                            i18n::status_label(self.lang, &status),
                            status_tone(&status),
                        )),
                )
                .child(
                    div()
                        .text_size(px(T11))
                        .font_family(FONT_MONO)
                        .text_color(c(FG2))
                        .truncate()
                        .child(tid.clone()),
                );
            let brief = brief.clone();
            list = list.child(row.on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                ws.open_library_reader(brief.clone(), cx);
            })));
        }
        pane = pane.child(list);
        pane
    }

    /// Event quadrant (F12): a true importance × urgency matrix — axis
    /// labels, cells that fill the canvas, P1 tinted, empty states written,
    /// cards that open the review drawer in place.
    fn render_quadrant(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        pane_w: gpui::Pixels,
    ) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let now = chrono::Local::now().naive_local();
        // Two fixed columns: label gutter + two cells spanning the pane.
        let cell_w = ((pane_w - px(32.0 + 64.0 + 16.0)) / 2.0).max(px(280.0));
        let mut cells: Vec<Vec<TaskSummary>> = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for t in &self.queue {
            cells[quadrant_of(&t.priority, &t.updated_at, &now)].push(t.clone());
        }
        let titles = [ts.q_p1, ts.q_p2, ts.q_p3, ts.q_p4];
        let tones = [c(RED), c(AMBER), c(ACCENT), c(FG3)];
        let row_labels = [ts.q_axis_important, ts.q_axis_minor];
        let col_labels = [ts.q_axis_urgent, ts.q_axis_not_urgent];

        let mut grid = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_2()
            .px_4()
            .pt_1()
            .id("quadrant-scroll")
            .overflow_y_scroll();

        // Axis headers: urgency across the top.
        grid = grid.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(div().w(px(64.0)))
                .child(
                    div()
                        .w(cell_w)
                        .text_size(px(T11))
                        .text_color(c(FG3))
                        .child(col_labels[0]),
                )
                .child(
                    div()
                        .w(cell_w)
                        .text_size(px(T11))
                        .text_color(c(FG3))
                        .child(col_labels[1]),
                ),
        );

        for row in 0..2 {
            let mut row_el = div().flex().flex_row().flex_1().min_h_0().gap_2();
            // Importance down the left.
            row_el = row_el.child(
                div()
                    .w(px(64.0))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(T11))
                            .text_color(c(FG3))
                            .child(row_labels[row]),
                    ),
            );
            for col in 0..2 {
                let idx = row * 2 + col;
                // P1 carries a faint red wash — the "look here first" zone.
                let cell_bg = if idx == 0 { c(RED_TINT) } else { c(SURFACE) };
                let mut cell = div()
                    .flex()
                    .flex_col()
                    .w(cell_w)
                    .min_w(cell_w)
                    .min_h(px(170.0))
                    .gap_1p5()
                    .rounded_lg()
                    .bg(cell_bg)
                    .border_1()
                    .border_color(c(HAIRLINE))
                    .p_3();
                cell = cell.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(tones[idx]))
                        .child(
                            div()
                                .text_size(px(T12))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(c(FG2))
                                .child(match self.lang {
                                    Lang::Zh => format!("{}（{}）", titles[idx], cells[idx].len()),
                                    Lang::En => format!("{} ({})", titles[idx], cells[idx].len()),
                                }),
                        ),
                );
                if cells[idx].is_empty() {
                    // Empty on purpose, and saying so.
                    cell = cell.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(T12))
                                    .text_color(c(FG3))
                                    .child(if idx == 0 { ts.q_empty_p1 } else { ts.q_empty }),
                            ),
                    );
                }
                for t in &cells[idx] {
                    let tid = t.task_id.clone();
                    let criterion = format!(
                        "{} · {}",
                        t.priority,
                        human_time(self.lang, &t.updated_at)
                    );
                    // Two-line card: identity + action, then the criteria.
                    let mut card = div()
                        .id(sid(format!("q-{tid}")))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .px_2p5()
                        .py_1p5()
                        .rounded_md()
                        .bg(c(RAISED))
                        .hover(|s| s.bg(c(RAISED_HOVER)))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(T12))
                                        .text_color(c(FG))
                                        .truncate()
                                        .child(t.object_name.clone()),
                                )
                                .child(self.button(
                                    ts.q_review,
                                    Btn::Ghost,
                                    None,
                                    window,
                                    cx,
                                    move |ws: &mut Workspace,
                                          _e: &ClickEvent,
                                          _w: &mut Window,
                                          cx: &mut Context<Workspace>| {
                                        cx.stop_propagation();
                                        ws.open_task(&tid, cx);
                                    },
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(T11))
                                        .font_family(FONT_MONO)
                                        .text_color(c(FG3))
                                        .child(t.period.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(T11))
                                        .font_family(FONT_MONO)
                                        .text_color(c(FG3))
                                        .truncate()
                                        .child(criterion),
                                )
                                .child(Badge::new(
                                    i18n::status_label(self.lang, &t.status),
                                    status_tone(&t.status),
                                )),
                        );
                    let tid = t.task_id.clone();
                    card = card.on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                        ws.open_task(&tid, cx);
                    }));
                    cell = cell.child(card);
                }
                row_el = row_el.child(cell);
            }
            grid = grid.child(row_el);
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                div().flex().flex_col().gap_1().px_5().pt_4().pb_2().child(
                    div()
                        .text_size(px(T15))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(c(FG))
                        .child(ts.quadrant_title),
                ),
            )
            .child(grid)
    }

    /// Audit trail (F7): quiet rows with verdict dot.
    fn render_audit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let records = self.audit.list();

        let mut pane = div().flex().flex_col().flex_1().min_h_0();
        pane = pane.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_5()
                .pt_4()
                .pb_3()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(T15))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(c(FG))
                                .child(match self.lang {
                                    Lang::Zh => format!("审核台账（F7 · {} 条）", records.len()),
                                    Lang::En => format!("Audit trail (F7 · {})", records.len()),
                                }),
                        )
                        .child(self.button(
                            ts.audit_export,
                            Btn::Ghost,
                            Some(GLYPH_EXPORT),
                            window,
                            cx,
                            |ws, _e, _w, cx| ws.export_audit(cx),
                        )),
                )
                .child(
                    div()
                        .text_size(px(T12))
                        .text_color(c(FG2))
                        .child(ts.audit_hint),
                ),
        );

        let mut list = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px_2()
            .id("audit-list")
            .overflow_y_scroll();
        if records.is_empty() {
            list = list.child(
                div()
                    .p_8()
                    .text_size(px(T13))
                    .text_color(c(FG2))
                    .child(ts.audit_empty),
            );
        }
        let n = records.len();
        for (idx, r) in records.iter().enumerate() {
            let is_last = idx + 1 == n;
            let mut row = div().flex().flex_col().gap_0p5().px_3().py_2().rounded_md();
            row = if is_last {
                row.hover(|s| s.bg(c(RAISED_HOVER)))
            } else {
                row.border_b_1()
                    .border_color(c(HAIRLINE))
                    .hover(|s| s.bg(c(RAISED_HOVER)).border_color(c(HAIRLINE)))
            };
            let row = row
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2p5()
                        .child(Badge::new(
                            i18n::verdict_label(self.lang, &r.verdict),
                            verdict_tone(&r.verdict),
                        ))
                        .child(
                            div()
                                .text_size(px(T13))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(c(FG))
                                .child(format!("{} {}", r.object_name, r.period)),
                        )
                        .child(
                            div()
                                .text_size(px(T11))
                                .font_family(FONT_MONO)
                                .text_color(c(FG3))
                                .child(format!("{} · {}", r.review_id, r.status_after)),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(T11))
                                .font_family(FONT_MONO)
                                .text_color(c(FG3))
                                .child(r.ts.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(T11))
                        .font_family(FONT_MONO)
                        .text_color(c(FG3))
                        .truncate()
                        .child(r.task_id.clone()),
                );
            list = list.child(row);
        }
        pane = pane.child(list);
        pane
    }
}

// Button styles used by both workspace and detail views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Primary,
    /// Solid green for the semantic "pass" decision.
    Success,
    Secondary,
    Ghost,
    DangerGhost,
}

/// Library/inbox status filter matching: both sides run through the same
/// label table, so contract enums match the vault's Chinese frontmatter.
fn status_matches(filter: &str, status: &str) -> bool {
    i18n::status_label(Lang::Zh, filter) == i18n::status_label(Lang::Zh, status)
}

/// Human-readable timestamp for list rows ("11-02 07:17" / "Nov 02, 07:17");
/// full ISO precision stays in the detail view where audits need it.
fn human_time(lang: Lang, raw: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S")
        .map(|t| match lang {
            Lang::Zh => t.format("%m-%d %H:%M").to_string(),
            Lang::En => t.format("%b %d, %H:%M").to_string(),
        })
        .unwrap_or_else(|_| raw.to_string())
}

/// Port of the research API base URL, for the sidebar's quiet meta line
/// (`api :8012 · I1 :8898`) — full URLs overflow a 232px column mid-value.
fn api_port(base: &str) -> &str {
    let rest = base
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = rest.split('/').next().unwrap_or(rest);
    host.rsplit(':').next().unwrap_or(host)
}

const GLYPH_INBOX: &str = "\u{E715}";
const GLYPH_LIB: &str = "\u{E8F1}";
const GLYPH_QUAD: &str = "\u{E9D9}";
const GLYPH_HIST: &str = "\u{E81C}";
const GLYPH_REFRESH: &str = "\u{E72C}";
const GLYPH_EXPORT: &str = "\u{E898}";

/// Stable idempotency key for the I3 write path: one review attempt per task
/// and brief version. Shared by the review modal and the submit handler.
pub fn review_idempotency_key(task_id: &str, brief_version: u64) -> String {
    format!("{task_id}:v{brief_version}")
}

impl gpui::Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_root(window, cx)
    }
}
