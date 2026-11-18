//! Workspace: root GPUI view wiring all WB functions —
//! review inbox (F5), brief library (F1), event quadrant (F12), audit trail (F7),
//! service status / degraded display (F10), webhook toasts (F9/I1).

use crate::chat::{self, ChatState, EngineState, ServerState};
use crate::i18n::{self, Lang};
use crate::input::TextInput;
use crate::theme::{sid, *};
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Entity, Hsla, MouseButton, MouseDownEvent, Window};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
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
use wb_core::duplicate::{self, BriefIdentity};
use wb_core::presummary::{self, SectionId};
use wb_core::provenance::{self, LabelledEvidence};
use wb_core::research::{ApprovalPolicy, ResearchClient, ResearchError};
use wb_core::state::quadrant_of;
use wb_core::structural::{self, StructuralReport};
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
        ShowAudit,
        ShowAnalytics,
        ToggleChat
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Inbox,
    Library,
    Quadrant,
    AuditTrail,
    Analytics,
}

impl ViewKind {
    pub fn label(&self) -> &'static str {
        match self {
            ViewKind::Inbox => "审核收件箱",
            ViewKind::Library => "简报库",
            ViewKind::Quadrant => "事件四象限",
            ViewKind::AuditTrail => "审核台账",
            ViewKind::Analytics => "审核分析",
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
    /// Inbox ordering. `Quadrant` is the §2.3 importance×urgency rule computed
    /// here from each task's own fields; `Fifo` is arrival order, the control
    /// condition for the §2.4b comparison. The ordering is a view concern, so it
    /// lives in the app and never travels over the wire.
    pub sort_mode: wb_core::state::SortMode,
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
    /// Structural accounting checks over the CALC section (F4 layer 2).
    pub structural: Option<StructuralReport>,
    /// F4 layer 3: each number resolved to the source sentence that supports it.
    /// Advisory — it explains the guardrail's verdict, it never produces one.
    pub citations: Vec<wb_core::provenance::Citation>,
    /// Other briefs describing the same event or restating the same figures.
    pub duplicates: Vec<wb_core::duplicate::Duplicate>,
    /// What changed in this version, computed from the diff and the CALC deltas.
    pub presage: Option<wb_core::presummary::Summary>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    pub show_evidence: bool,
    pub hovered_violation: Option<usize>,

    /// AI side chat (F14): local llama.cpp engine + 2–6B GGUF models.
    pub chat: ChatState,

    pub review: Option<ReviewFlow>,
    pub toasts: Vec<Toast>,
    /// Keyboard-shortcuts modal, toggled from the topbar ? button.
    pub show_shortcuts: bool,
    /// Collapsed system/audit metadata accordion on the detail page.
    pub show_meta: bool,
    /// Collapsed process-timeline accordion on the detail page.
    pub show_history: bool,

    /// 任务行右键菜单：Some((task_id, status, 点击位置))；Esc/点他处/选动作后关闭。
    pub ctx_menu_task: Option<(String, String, gpui::Point<gpui::Pixels>)>,
    /// 右键菜单"通过/驳回"的两段式动作：先 open_task 加载详情，完成后自动开审核弹窗。
    pub pending_verdict: Option<(String, wb_core::model::Verdict)>,
    /// 菜单栏当前展开项（"file"/"edit"/"view"/"help"）与下拉锚定 x（点击位置）。
    pub open_menu: Option<&'static str>,
    pub menu_anchor_x: gpui::Pixels,

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
        let chat_input = cx.new(|cx| TextInput::new(false, ts0.chat_input_placeholder, false, cx));

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
            sort_mode: wb_core::state::SortMode::Quadrant,
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
            structural: None,
            detail_loading: false,
            detail_error: None,
            citations: Vec::new(),
            duplicates: Vec::new(),
            presage: None,
            show_evidence: false,
            hovered_violation: None,
            chat: ChatState::from_input(chat_input.clone()),
            review: None,
            toasts: Vec::new(),
            show_shortcuts: false,
            show_meta: false,
            show_history: false,
            ctx_menu_task: None,
            pending_verdict: None,
            open_menu: None,
            menu_anchor_x: px(0.0),
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
        ws.spawn_chat_loop(cx);
        // Quit hygiene: kill the llama-server child AND the curl download
        // children — otherwise curl keeps pulling multi-GB files after the
        // app is gone and the .part is never finalized.
        let _ = cx.on_app_quit(|ws, _cx| {
            if let Ok(mut g) = ws.chat.child_slot.lock() {
                if let Some(c) = g.as_mut() {
                    let _ = c.kill();
                }
                *g = None;
            }
            for job in ws.chat.downloads.values() {
                job.cancel();
            }
            async {}
        });
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
                        // F4 layer 2: re-compute derived indicators from the
                        // CALC section's own numbers (pure arithmetic).
                        ws.structural = Some(structural::check(&d.sections.calculation.0, &guard_cfg));

                        // F4 layer 3 (advisory): resolve each number to the source
                        // sentence that supports it. Built from the same inputs as
                        // the guardrail, so a number can never be cited by one and
                        // flagged by the other.
                        let mut labelled = LabelledEvidence::default();
                        if let Ok(ctx) = &context {
                            labelled.metrics = ctx
                                .metrics
                                .0
                                .iter()
                                .filter_map(|(k, v)| v.as_f64().map(|v| (k.clone(), v)))
                                .collect();
                            labelled
                                .excerpts
                                .push(("公告摘要".into(), ctx.announcement.summary.clone()));
                        }
                        if let Ok(ev) = &evidence {
                            labelled
                                .excerpts
                                .push((ev.locator.clone(), ev.excerpt.clone()));
                        }
                        if let Some(brief) = vault.read_brief(&tid) {
                            labelled.calc = brief.calc_values().into_iter().collect();
                        }
                        ws.citations = provenance::cite(&d.sections.inference, &labelled, &guard_cfg);

                        // Duplicate / restatement detection over the vault.
                        ws.duplicates = duplicates_for(&vault, &tid, &d, &guard_cfg);

                        // What changed since v1, when a preserved draft exists.
                        ws.presage = presummary_for(&vault, &tid, &d, &guard_cfg);
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
                        // 右键菜单"通过/驳回"两段式：详情就绪后自动开审核弹窗
                        if let Some((vtid, verdict)) = ws.pending_verdict.take() {
                            if vtid == tid {
                                ws.open_review(verdict, cx);
                            }
                        }
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
                                ws.structural = Some(structural::check(&d.sections.calculation.0, &Default::default()));
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
        // The revision text is moved into the request; analytics needs a copy.
        let revised_for_audit = revised.clone();

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
                    if let Err(e) = ws.audit.record_review_ex(
                        &req_for_audit,
                        &object_name,
                        &period,
                        &receipt,
                        // Capture the review-assist context for analytics (F13):
                        // what the guardrail flagged, what the reviewer saw and
                        // what they changed.
                        &ws.guardrail
                            .as_ref()
                            .map(|g| g.violation_tokens())
                            .unwrap_or_default(),
                        ws.detail.as_ref().map(|d| d.sections.inference.clone()),
                        if req_for_audit.verdict == Verdict::ApproveAsRevised {
                            revised_for_audit.clone()
                        } else {
                            None
                        },
                    )
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

// ============================================================ AI side chat

impl Workspace {
    fn spawn_chat_loop(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            this.update(cx, |ws, cx| {
                ws.chat_tick(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Drain background chat work into UI state. Cheap when idle.
    fn chat_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = self.chat.tick_probe();

        // 1. streaming deltas → the partial bubble / final message
        if let Some(shared) = self.chat.stream.clone() {
            let snap = shared.snapshot();
            let partial = (!snap.partial.is_empty()).then(|| snap.partial.clone());
            if self.chat.streaming != partial {
                self.chat.streaming = partial;
                changed = true;
            }
            if snap.done {
                self.chat.stream = None;
                self.chat.streaming = None;
                match snap.error {
                    Some(e) => {
                        self.chat.error = Some(e);
                    }
                    None => {
                        if snap.partial.trim().is_empty() {
                            // The question must never just vanish.
                            self.chat.error = Some(match self.lang {
                                Lang::Zh => "模型没有返回内容，请重试。".to_string(),
                                Lang::En => "The model returned nothing — try again.".to_string(),
                            });
                        } else {
                            self.chat
                                .messages
                                .push(chat::ChatMessage::assistant(snap.partial));
                        }
                    }
                }
                changed = true;
            }
        }

        // 2. download progress (file size of the .part) + terminal states
        let mut finished: Vec<&'static str> = Vec::new();
        for (id, job) in &self.chat.downloads {
            let state = job.state();
            if let Ok(size) = std::fs::metadata(&job.dest_part) {
                let n = size.len();
                if job.got.lock().map(|mut g| {
                    if *g != n {
                        *g = n;
                        true
                    } else {
                        false
                    }
                }).unwrap_or(false) {
                    changed = true;
                }
            }
            if !matches!(state, chat::DownloadState::Running) {
                changed = true;
                // Terminal states leave the map entirely: readiness is
                // derived from the file on disk, and lingering entries would
                // block a later re-download via chat_begin_download's guard.
                finished.push(id);
            }
        }
        for id in finished {
            self.chat.downloads.remove(id);
        }

        // 3. server lifecycle while Starting: fast-fail when the process
        // already died (bad GGUF, port busy), health polling otherwise.
        if matches!(self.chat.server_state, ServerState::Starting) {
            let mut died = false;
            if let Ok(mut slot) = self.chat.child_slot.lock() {
                if let Some(c) = slot.as_mut() {
                    if matches!(c.try_wait(), Ok(Some(_))) {
                        died = true;
                    }
                }
            }
            if died {
                if let Ok(mut slot) = self.chat.child_slot.lock() {
                    *slot = None;
                }
                let msg = match self.lang {
                    Lang::Zh => "模型进程已退出（详见 wb-data/chat-server.log）".to_string(),
                    Lang::En => {
                        "Model process exited (see wb-data/chat-server.log)".to_string()
                    }
                };
                self.chat.server_state = ServerState::Failed(msg);
                changed = true;
            } else if let Some(server) = &self.chat.server {
                if !self.chat.health_pending.swap(true, Ordering::Relaxed) {
                    let slot = self.chat.health_slot.clone();
                    let port = server.port;
                    cx.background_executor()
                        .spawn(async move {
                            let ok = chat::server_listening(port);
                            if let Ok(mut g) = slot.lock() {
                                *g = Some(ok);
                            }
                        })
                        .detach();
                }
            }
            let polled = self
                .chat
                .health_slot
                .lock()
                .map(|mut g| g.take())
                .unwrap_or(None);
            if let Some(ok) = polled {
                self.chat.health_pending.store(false, Ordering::Relaxed);
                if ok {
                    self.chat.server_state = ServerState::Ready;
                } else if self
                    .chat
                    .server
                    .as_ref()
                    .map(|s| s.started_at.elapsed() > Duration::from_secs(180))
                    .unwrap_or(false)
                {
                    // Reclaim the port/VRAM: kill the child on timeout.
                    if let Ok(mut slot) = self.chat.child_slot.lock() {
                        if let Some(c) = slot.as_mut() {
                            let _ = c.kill();
                        }
                        *slot = None;
                    }
                    let msg = match self.lang {
                        Lang::Zh => "模型加载超时（180s），已停止".to_string(),
                        Lang::En => "Model load timed out (180s), stopped".to_string(),
                    };
                    self.chat.server_state = ServerState::Failed(msg);
                }
                changed = true;
            }
        }

        if changed {
            cx.notify();
        }
        changed
    }

    pub fn chat_toggle(&mut self, cx: &mut Context<Self>) {
        self.chat.open = !self.chat.open;
        if self.chat.open {
            let models_dir = self.cfg.models_dir_or_default();
            self.chat.ensure_probed(&self.cfg.data_dir, &models_dir, cx);
        }
        cx.notify();
    }

    /// Full engine re-probe: instant checks again, and if they miss, restart
    /// the background full-machine sweep.
    pub fn chat_recheck(&mut self, cx: &mut Context<Self>) {
        self.chat.engine = chat::find_llama_server(&self.cfg.data_dir).map(EngineState::Ready);
        if self.chat.engine.is_some() {
            self.chat.catalog_view = true;
        } else {
            self.chat.start_deep_scan();
        }
        cx.notify();
    }

    pub fn chat_begin_download(&mut self, model_id: &'static str, cx: &mut Context<Self>) {
        let Some(m) = chat::model_by_id(model_id) else { return };
        if let Some(existing) = self.chat.downloads.get(model_id) {
            match existing.state() {
                chat::DownloadState::Running => return,
                _ => {
                    self.chat.downloads.remove(model_id);
                }
            }
        }
        let dir = self.cfg.models_dir_or_default();
        match chat::start_download(m, &dir) {
            Ok(job) => {
                self.chat.downloads.insert(model_id, job);
                cx.notify();
            }
            Err(e) => self.push_toast(e, ToastKind::Error, cx),
        }
    }

    pub fn chat_cancel_download(&mut self, model_id: &'static str, cx: &mut Context<Self>) {
        if let Some(job) = self.chat.downloads.get(model_id) {
            job.cancel();
        }
        cx.notify();
    }

    pub fn chat_start_model(&mut self, model_id: &'static str, cx: &mut Context<Self>) {
        let Some(m) = chat::model_by_id(model_id) else { return };
        if self.chat.engine.is_none() {
            return;
        }
        let dest = self.cfg.models_dir_or_default().join(m.filename);
        if !dest.is_file() {
            self.push_toast(
                format!("{}：{}", m.name, m.filename),
                ToastKind::Warn,
                cx,
            );
            return;
        }
        self.chat_start_local(dest, m.name.to_string(), cx);
    }

    /// Start a server from any GGUF already on disk (Ollama / LM Studio /
    /// manual downloads). One model at a time: stops the current server.
    pub fn chat_start_local(&mut self, path: PathBuf, name: String, cx: &mut Context<Self>) {
        let Some(EngineState::Ready(exe)) = self.chat.engine.clone() else {
            return;
        };
        if !path.is_file() {
            return;
        }
        self.chat_stop_server(cx);
        let log = self.cfg.data_dir.join("chat-server.log");
        std::fs::create_dir_all(&self.cfg.data_dir).ok();
        match chat::spawn_llama_server(
            &exe,
            &path,
            name,
            self.cfg.chat_port,
            &log,
            &self.chat.child_slot,
        ) {
            Ok(handle) => {
                self.chat.server = Some(handle);
                self.chat.server_state = ServerState::Starting;
                self.chat.error = None;
            }
            Err(e) => {
                self.chat.server_state = ServerState::Failed(e.clone());
                self.push_toast(e, ToastKind::Error, cx);
            }
        }
        cx.notify();
    }

    pub fn chat_stop_server(&mut self, cx: &mut Context<Self>) {
        if let Ok(mut slot) = self.chat.child_slot.lock() {
            if let Some(c) = slot.as_mut() {
                let _ = c.kill();
            }
            *slot = None;
        }
        if self.chat.server.is_some() || self.chat.server_state != ServerState::Stopped {
            self.chat.server = None;
            self.chat.server_state = ServerState::Stopped;
            cx.notify();
        }
    }

    /// Send the chat input as a user message against the running model.
    pub fn chat_send(&mut self, cx: &mut Context<Self>) {
        let text = self.chat.input.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        if !matches!(self.chat.server_state, ServerState::Ready) {
            self.chat.error = Some(i18n::strings(self.lang).chat_need_model.to_string());
            cx.notify();
            return;
        }
        self.chat.messages.push(chat::ChatMessage::user(text));
        self.chat.input.update(cx, |ti, cx| {
            ti.content.clear();
            ti.cursor = 0;
            cx.notify();
        });

        let port = self.chat.server.as_ref().map(|s| s.port).unwrap_or(self.cfg.chat_port);
        let mut outgoing = Vec::new();
        // Task-aware mode: the open task's brief, guardrail and structural
        // results become the system prompt.
        if let Some(d) = self.detail.clone() {
            outgoing.push(chat::ChatMessage::system(chat::build_system_context_ext(
                self.lang,
                &d,
                self.guardrail.as_ref(),
                self.structural.as_ref(),
                self.evidence.as_ref(),
                &self.citations,
                &self.duplicates,
            )));
        }
        outgoing.extend(self.chat.messages.clone());
        let shared = chat::StreamShared::new();
        self.chat.stream = Some(shared.clone());
        self.chat.streaming = Some(String::new());
        self.chat.error = None;
        cx.notify();

        // Network on a plain thread; deltas land in the shared buffer and the
        // chat loop drains them into the entity.
        std::thread::spawn(move || {
            let mut sink = String::new();
            let res = chat::chat_stream(port, &outgoing, &mut sink, &shared.stop);
            if let Ok(mut inner) = shared.inner.lock() {
                inner.partial = sink;
                inner.error = res.err();
                inner.done = true;
            }
        });
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
                ViewKind::Analytics => div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_analytics(cx))
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
                if ws.open_menu.is_some() {
                    // Esc 优先关闭菜单栏下拉
                    ws.open_menu = None;
                } else if ws.ctx_menu_task.is_some() {
                    // 其次关闭右键菜单，不触发返回
                    ws.ctx_menu_task = None;
                } else if ws.show_shortcuts {
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
                        ws.structural = None;
                        ws.show_evidence = false;
                    }
                }
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &SubmitReview, _w, cx| {
                if ws.review.is_some() {
                    ws.submit_review(cx);
                } else if ws.chat.open {
                    // Ctrl+Enter in the side chat sends the message.
                    ws.chat_send(cx);
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
            .on_action(cx.listener(|ws, _: &ShowAnalytics, _w, cx| {
                ws.view = ViewKind::Analytics;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ToggleChat, _w, cx| {
                ws.chat_toggle(cx);
            }))
            // 左键点击任意空白处关闭展开的菜单栏下拉（菜单项/菜单栏自身均 stop_propagation）
            .on_mouse_down(MouseButton::Left, cx.listener(|ws, _ev, _w, cx| {
                if ws.open_menu.is_some() {
                    ws.open_menu = None;
                    cx.notify();
                }
            }))
            .child(self.render_topbar(window, cx))
            .child(self.render_menubar(cx))
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
                    .child(main_pane)
                    .when(self.chat.open, |el| el.child(self.render_chat_panel(window, cx))),
            )
            .when(self.ctx_menu_task.is_some(), |el| {
                el.child(self.render_task_ctx_menu(cx))
            })
            .when(self.open_menu.is_some(), |el| {
                el.child(self.render_app_menu(window, cx))
            })
    }

    /// 任务行右键菜单（deferred 浮层，锚定右键点击位置；Esc/点他处/选动作后关闭）。
    fn render_task_ctx_menu(&mut self, cx: &mut Context<Self>) -> gpui::Deferred {
        let (mtid, mstatus, p) = self.ctx_menu_task.clone().unwrap();
        let can_review = mstatus == "WAITING_REVIEW";
        let mk = move |label: &'static str, enabled: bool, act: &'static str, tid: String, cx: &mut Context<Self>| {
            let mut d = div().px_3().py_1().text_size(px(12.0)).child(label);
            if enabled {
                d = d
                    .text_color(c(FG))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ACCENT_DIM)))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |ws, _ev, _w, cx| {
                        ws.ctx_menu_task = None;
                        match act {
                            "open" => ws.open_task(&tid, cx),
                            "approve" => {
                                ws.pending_verdict = Some((tid.clone(), Verdict::Approve));
                                ws.open_task(&tid, cx);
                            }
                            "reject" => {
                                ws.pending_verdict = Some((tid.clone(), Verdict::Reject));
                                ws.open_task(&tid, cx);
                            }
                            "copy" => {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(tid.clone()));
                                ws.push_toast("任务 ID 已复制".to_string(), ToastKind::Info, cx);
                            }
                            _ => {}
                        }
                        cx.notify();
                        cx.stop_propagation();
                    }));
            } else {
                d = d.text_color(c(DIM));
            }
            d
        };
        gpui::deferred(
            div()
                .absolute()
                .left(p.x)
                .top(p.y)
                .flex()
                .flex_col()
                .min_w(px(140.0))
                .bg(c(SURFACE))
                .border_1()
                .border_color(c(BORDER))
                .rounded_md()
                .shadow_lg()
                .child(mk("打开详情", true, "open", mtid.clone(), cx))
                .child(mk("通过", can_review, "approve", mtid.clone(), cx))
                .child(mk("驳回", can_review, "reject", mtid.clone(), cx))
                .child(mk("复制任务 ID", true, "copy", mtid.clone(), cx))
                .child(mk("取消", true, "cancel", mtid.clone(), cx)),
        )
    }

    // ------------------------------------------------------------- 菜单栏（Windows 风格）

    /// 当前聚焦的文本输入框（编辑菜单的转发目标）。
    fn focused_input(&self, window: &Window, cx: &App) -> Option<Entity<TextInput>> {
        let mut cands: Vec<Entity<TextInput>> =
            vec![self.company_input.clone(), self.search_input.clone()];
        if let Some(r) = &self.review {
            cands.push(r.comment.clone());
            if let Some(e) = &r.editor {
                cands.push(e.clone());
            }
        }
        cands.into_iter().find(|e| e.read(cx).focus.is_focused(window))
    }

    fn menu_forward_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.focused_input(window, cx) {
            input.update(cx, |i, cx| i.menu_paste(window, cx));
        }
    }

    fn menu_forward_cut_copy(&mut self, cut: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.focused_input(window, cx) {
            input.update(cx, |i, cx| i.menu_cut_copy(cut, window, cx));
        }
    }

    fn menu_forward_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.focused_input(window, cx) {
            input.update(cx, |i, cx| i.select_all(cx));
        }
    }

    /// 菜单栏（顶栏下方）：文件/编辑/视图/帮助；点击展开，悬停切换，Esc/点他处关闭。
    fn render_menubar(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mk = |label: &'static str, kind: &'static str, cx: &mut Context<Self>| {
            let open = self.open_menu == Some(kind);
            div()
                .id(sid(format!("menu-{kind}")))
                .px_3()
                .h_full()
                .flex()
                .items_center()
                .text_size(px(12.0))
                .text_color(if open { c(ACCENT) } else { c(FG2) })
                .when(open, |el| el.bg(c(ACCENT_DIM)))
                .hover(|s| s.bg(c(ACCENT_DIM)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                    if ws.open_menu == Some(kind) {
                        ws.open_menu = None;
                    } else {
                        ws.open_menu = Some(kind);
                        ws.menu_anchor_x = ev.position.x;
                    }
                    cx.notify();
                    cx.stop_propagation();
                }))
                .on_hover({
                    // 菜单已展开时悬停切换（Windows 行为）；on_hover 是纯闭包，
                    // 经实体句柄回写状态
                    let entity = cx.entity();
                    move |hovered: &bool, _w, app: &mut App| {
                        if *hovered {
                            entity.update(app, |ws, cx| {
                                if ws.open_menu.is_some() && ws.open_menu != Some(kind) {
                                    ws.open_menu = Some(kind);
                                    cx.notify();
                                }
                            });
                        }
                    }
                })
                .child(label)
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(28.0))
            .px_2()
            .bg(c(SURFACE))
            .border_b_1()
            .border_color(c(HAIRLINE))
            .child(mk(ts.menu_file, "file", cx))
            .child(mk(ts.menu_edit, "edit", cx))
            .child(mk(ts.menu_view, "view", cx))
            .child(mk(ts.menu_help, "help", cx))
    }

    fn menu_action_row(
        &self,
        label: &'static str,
        enabled: bool,
        act: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut d = div().px_3().py_1().text_size(px(12.0)).child(label);
        if enabled {
            d = d
                .text_color(c(FG))
                .cursor_pointer()
                .hover(|s| s.bg(c(ACCENT_DIM)))
                .on_mouse_down(MouseButton::Left, cx.listener(move |ws, _ev, window, cx| {
                    ws.open_menu = None;
                    match act {
                        "refresh" => ws.reload_queue(cx),
                        "export" => ws.export_audit(cx),
                        "quit" => cx.quit(),
                        "cut" => ws.menu_forward_cut_copy(true, window, cx),
                        "copy" => ws.menu_forward_cut_copy(false, window, cx),
                        "paste" => ws.menu_forward_paste(window, cx),
                        "selall" => ws.menu_forward_select_all(window, cx),
                        "v-inbox" => ws.view = ViewKind::Inbox,
                        "v-library" => ws.view = ViewKind::Library,
                        "v-quadrant" => ws.view = ViewKind::Quadrant,
                        "v-audit" => ws.view = ViewKind::AuditTrail,
                        "v-analytics" => ws.view = ViewKind::Analytics,
                        "chat" => ws.chat_toggle(cx),
                        "lang" => ws.toggle_language(cx),
                        "shortcuts" => ws.show_shortcuts = true,
                        "about" => {
                            let ts = i18n::strings(ws.lang);
                            ws.push_toast(ts.about_text.to_string(), ToastKind::Info, cx);
                        }
                        _ => {}
                    }
                    cx.notify();
                    cx.stop_propagation();
                }));
        } else {
            d = d.text_color(c(DIM));
        }
        d
    }

    /// 菜单栏下拉（deferred 浮层，锚定菜单栏下缘 y=44+28=72px）。
    fn render_app_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Deferred {
        let ts = i18n::strings(self.lang);
        let kind = self.open_menu.unwrap();
        let x = self.menu_anchor_x;
        // 编辑菜单的启停状态依赖当前聚焦输入框
        let focused = self.focused_input(window, cx);
        let focused_has_sel = focused
            .as_ref()
            .map(|e| e.read(cx).has_selection())
            .unwrap_or(false);
        let can_edit = focused.is_some();
        let items: Vec<(&'static str, bool, &'static str)> = match kind {
            "file" => vec![
                (ts.refresh_queue, true, "refresh"),
                (ts.audit_export, true, "export"),
                (ts.mi_quit, true, "quit"),
            ],
            "edit" => vec![
                (ts.mi_cut, focused_has_sel, "cut"),
                (ts.mi_copy, focused_has_sel, "copy"),
                (ts.mi_paste, can_edit, "paste"),
                (ts.mi_select_all, can_edit, "selall"),
            ],
            "view" => vec![
                (ts.nav_inbox, true, "v-inbox"),
                (ts.nav_library, true, "v-library"),
                (ts.nav_quadrant, true, "v-quadrant"),
                (ts.nav_audit, true, "v-audit"),
                (ts.nav_analytics, true, "v-analytics"),
                (ts.chat_title, true, "chat"),
                (ts.mi_language, true, "lang"),
                (ts.shortcuts_title, true, "shortcuts"),
            ],
            _ => vec![(ts.mi_about, true, "about")],
        };
        let mut menu = div()
            .absolute()
            .left(x)
            .top(px(72.0))
            .flex()
            .flex_col()
            .min_w(px(170.0))
            .bg(c(SURFACE))
            .border_1()
            .border_color(c(BORDER))
            .rounded_md()
            .shadow_lg();
        for (label, enabled, act) in items {
            menu = menu.child(self.menu_action_row(label, enabled, act, cx));
        }
        gpui::deferred(menu)
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
                ts.chat_title,
                Btn::Ghost,
                Some(GLYPH_CHAT),
                _w,
                cx,
                |ws, _e, _w, cx| {
                    ws.chat_toggle(cx);
                },
            ))
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
            (ViewKind::Analytics, GLYPH_CHART, ts.nav_analytics),
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
        let rows: [(&str, &str); 9] = [
            ("Ctrl+1", ts.sc_inbox),
            ("Ctrl+2", ts.sc_library),
            ("Ctrl+3", ts.sc_quadrant),
            ("Ctrl+4", ts.sc_audit),
            ("Ctrl+5", ts.sc_analytics),
            ("Ctrl+R", ts.sc_refresh),
            ("Esc", ts.sc_back),
            ("Ctrl+Enter", ts.sc_submit),
            ("Ctrl+L", ts.sc_chat),
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
            Btn::Primary => (c(ACCENT), c(0xffffff), c(0x1d4ed8)),
            Btn::Success => (c(GREEN), c(0xffffff), c(0x166b2a)),
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
                Btn::Primary | Btn::Success => c(0xffffff),
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
        // Ordering toggle: quadrant (importance × urgency) vs FIFO (arrival).
        // Both orderings exist in the product because §2.4b compares them; the
        // label shows the mode you are switching TO, matching the language
        // toggle's convention.
        let (sort_label, sort_active) = match self.sort_mode {
            wb_core::state::SortMode::Quadrant => (ts.sort_fifo, false),
            wb_core::state::SortMode::Fifo => (ts.sort_quadrant, true),
        };
        let mut sort_btn = div()
            .flex()
            .items_center()
            .h(px(24.0))
            .px_2p5()
            .rounded_md()
            .text_size(px(T12))
            .id("sort-toggle");
        sort_btn = if sort_active {
            sort_btn
                .bg(c(ACCENT_DIM))
                .text_color(c(ACCENT))
                .font_weight(gpui::FontWeight::MEDIUM)
        } else {
            sort_btn
                .text_color(c(FG2))
                .hover(|s| s.bg(c(RAISED)).text_color(c(FG)))
        };
        filter_row = filter_row
            .child(div().w(px(260.0)).ml_3().child(self.company_input.clone()))
            .child(div().flex_1())
            .child(sort_btn.child(sort_label).on_click(cx.listener(
                |ws, _e: &ClickEvent, _w, cx| {
                    ws.sort_mode = ws.sort_mode.toggle();
                    cx.notify();
                },
            )));
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
        //
        // Ordering is applied here, not taken from the wire: §2.4b compares
        // quadrant order against FIFO on the same queue, so both modes must run
        // through the same code with the mode as the only difference.
        let filter = self.status_filter.clone();
        let company = self.company_filter.to_lowercase();
        let visible: Vec<TaskSummary> = self
            .queue
            .iter()
            .filter(|t| filter.as_deref().map(|f| t.status == f).unwrap_or(true))
            .filter(|t| {
                company.is_empty()
                    || t.object_name.to_lowercase().contains(&company)
                    || t.object_id.to_lowercase().contains(&company)
                    || t.task_id.to_lowercase().contains(&company)
            })
            .cloned()
            .collect();
        let now = chrono::Local::now().naive_local();
        let triaged = crate::triage::triage(&visible, &self.vault, self.sort_mode, &now);
        let rows: Vec<TaskSummary> = triaged.into_iter().map(|r| r.task).collect();
        let rows: Vec<&TaskSummary> = rows.iter().collect();

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
            let tid_rc = tid.clone();
            let status_rc = t.status.clone();
            list = list.child(
                row.on_mouse_down(MouseButton::Right, cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                        // 任务行右键：展开操作菜单（打开详情/通过/驳回/复制任务ID）
                        ws.ctx_menu_task = Some((tid_rc.clone(), status_rc.clone(), ev.position));
                        cx.notify();
                        cx.stop_propagation();
                    }))
                    .on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                        ws.ctx_menu_task = None;
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

/// Identity of a vault brief, for duplicate comparison.
fn identity_of(brief: &Brief) -> BriefIdentity {
    let fm = &brief.frontmatter;
    BriefIdentity {
        task_id: fm.task_id.clone(),
        event_ref: fm.event_ref.clone(),
        object_id: fm.object_id.clone(),
        period: fm.period.clone(),
        calc: brief.calc_values(),
    }
}

/// Compare the open brief against every other brief in the vault.
///
/// Reads the vault rather than the queue: a duplicate is a property of the
/// documents, and a task that has left the queue (SUCCEEDED) still matters — its
/// brief is the one a later restatement would contradict.
fn duplicates_for(
    vault: &Vault,
    task_id: &str,
    detail: &TaskDetail,
    cfg: &GuardrailConfig,
) -> Vec<duplicate::Duplicate> {
    let me = match vault.read_brief(task_id) {
        Some(b) => identity_of(&b),
        // No vault file yet: fall back to the wire payload so a brief that has
        // not been written still gets checked against its predecessors.
        None => BriefIdentity {
            task_id: detail.task_id.clone(),
            event_ref: detail.object_id.clone(),
            object_id: detail.object_id.clone(),
            period: detail.period.clone(),
            calc: detail
                .sections
                .calculation
                .0
                .iter()
                .filter_map(|(k, v)| v.as_f64().map(|v| (k.clone(), v)))
                .collect(),
        },
    };
    let others: Vec<BriefIdentity> = vault.search("").iter().map(|(b, _)| identity_of(b)).collect();
    duplicate::find_duplicates(&me, &others, cfg)
}

/// What changed since the preserved v1 draft, when one exists.
///
/// `{task_id}.v1.md` is written by F6 on approve-as-revised, so its absence just
/// means this brief has never been revised — not an error.
fn presummary_for(
    vault: &Vault,
    task_id: &str,
    detail: &TaskDetail,
    cfg: &GuardrailConfig,
) -> Option<presummary::Summary> {
    let v1 = vault.read_brief_at(&format!("{task_id}.v1.md"))?;
    let sections = |b: &Brief| -> Vec<(SectionId, String)> {
        let get = |k: wb_core::vault::SectionKind| {
            b.section(k).map(|s| s.body.clone()).unwrap_or_default()
        };
        vec![
            (SectionId::Fact, get(wb_core::vault::SectionKind::Fact)),
            (
                SectionId::Calculation,
                get(wb_core::vault::SectionKind::Calculation),
            ),
            (
                SectionId::Inference,
                get(wb_core::vault::SectionKind::Inference),
            ),
            (SectionId::Check, get(wb_core::vault::SectionKind::Check)),
        ]
    };

    let after_sections = vec![
        (SectionId::Fact, detail.sections.fact.0.join("\n")),
        (
            SectionId::Calculation,
            detail
                .sections
                .calculation
                .0
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        (SectionId::Inference, detail.sections.inference.clone()),
        (SectionId::Check, detail.sections.check.clone()),
    ];

    let after_calc: std::collections::BTreeMap<String, f64> = detail
        .sections
        .calculation
        .0
        .iter()
        .filter_map(|(k, v)| v.as_f64().map(|v| (k.clone(), v)))
        .collect();
    let before_calc = v1.calc_values();

    let s = presummary::summarize(&sections(&v1), &after_sections, &before_calc, &after_calc);

    // Verdict delta, computed the same way for both versions so the comparison
    // is like-for-like.
    let verdict = |text: &str, calc: &std::collections::BTreeMap<String, f64>| -> Option<bool> {
        if text.trim().is_empty() {
            return None;
        }
        let mut inputs = EvidenceInputs::default();
        inputs.metrics.extend(calc.values().copied());
        Some(
            guardrail_check(text, &build_evidence_numbers(&inputs), cfg)
                .ok(),
        )
    };
    let before_ok = verdict(
        v1.section(wb_core::vault::SectionKind::Inference)
            .map(|s| s.body.as_str())
            .unwrap_or(""),
        &before_calc,
    );
    let after_ok = verdict(&detail.sections.inference, &after_calc);
    Some(presummary::with_verdicts(s, before_ok, after_ok))
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
const GLYPH_CHART: &str = "\u{E9D2}";
const GLYPH_CHAT: &str = "\u{E8F2}";
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
