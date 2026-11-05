//! UI language: 中文 / English. All UI chrome (nav, headers, badges, buttons,
//! forms, empty states, toasts) is localized; brief content and audit records
//! remain the artifacts they are (Chinese research text, contract verdicts).

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn toggle(self) -> Lang {
        match self {
            Lang::Zh => Lang::En,
            Lang::En => Lang::Zh,
        }
    }

    /// Label of the toggle button (shows the language you can switch TO).
    pub fn toggle_label(self) -> &'static str {
        match self {
            Lang::Zh => "EN",
            Lang::En => "中文",
        }
    }

    pub fn from_config(s: &str) -> Lang {
        match s {
            "en" | "EN" | "en-US" => Lang::En,
            _ => Lang::Zh,
        }
    }

    pub fn to_config(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }
}

pub struct Strings {
    pub app_title: &'static str,
    pub refresh_queue: &'static str,
    pub service_ok: &'static str,
    pub service_down: &'static str,
    pub connecting: &'static str,
    pub refreshed_at: &'static str,

    pub nav_inbox: &'static str,
    pub nav_library: &'static str,
    pub nav_quadrant: &'static str,
    pub nav_audit: &'static str,

    pub col_priority: &'static str,
    pub col_company: &'static str,
    pub col_period: &'static str,
    pub col_status: &'static str,
    pub col_time: &'static str,
    pub col_task: &'static str,
    pub meta_task: &'static str,
    pub meta_trace: &'static str,
    pub meta_object: &'static str,
    pub meta_workflow: &'static str,
    pub shortcuts_title: &'static str,
    pub sc_inbox: &'static str,
    pub sc_library: &'static str,
    pub sc_quadrant: &'static str,
    pub sc_audit: &'static str,
    pub sc_refresh: &'static str,
    pub sc_back: &'static str,
    pub sc_submit: &'static str,
    pub close: &'static str,
    pub expand_full: &'static str,
    pub meta_toggle: &'static str,
    pub reader_open_review: &'static str,
    pub reader_reviewer: &'static str,
    pub q_axis_urgent: &'static str,
    pub q_axis_not_urgent: &'static str,
    pub q_axis_important: &'static str,
    pub q_axis_minor: &'static str,
    pub q_empty_p1: &'static str,
    pub q_empty: &'static str,
    pub q_review: &'static str,

    pub inbox_title: &'static str,
    pub inbox_hint: &'static str,
    pub all: &'static str,
    pub company_filter_placeholder: &'static str,
    pub loading_queue: &'static str,
    pub queue_degraded: &'static str,

    pub library_title: &'static str,
    pub library_search_placeholder: &'static str,
    pub library_empty: &'static str,

    pub quadrant_title: &'static str,
    pub q_p1: &'static str,
    pub q_p2: &'static str,
    pub q_p3: &'static str,
    pub q_p4: &'static str,

    pub audit_export: &'static str,
    pub audit_hint: &'static str,
    pub audit_empty: &'static str,

    pub back: &'static str,
    pub brief_suffix: &'static str,
    pub cutoff_stale: &'static str,
    pub detail_loading: &'static str,
    pub detail_failed: &'static str,
    pub not_reviewable: &'static str,
    pub approve: &'static str,
    pub approve_as_revised: &'static str,
    pub reject: &'static str,
    pub review_via_mcp: &'static str,
    pub evidence_toggle: &'static str,
    pub evidence_hide: &'static str,
    pub evidence_sub: &'static str,
    pub evidence_title: &'static str,
    pub evidence_excerpt: &'static str,
    pub evidence_announcement: &'static str,
    pub evidence_methodology: &'static str,
    pub history_title: &'static str,

    pub sec_fact: &'static str,
    pub sec_calc: &'static str,
    pub sec_inference: &'static str,
    pub sec_check: &'static str,
    pub guard_ok: &'static str,
    pub guard_fail: &'static str,

    pub review_title: &'static str,
    pub comment_label: &'static str,
    pub comment_placeholder: &'static str,
    pub revise_label: &'static str,
    pub revise_placeholder: &'static str,
    pub diff_same: &'static str,
    pub diff_changed: &'static str,
    pub revise_empty: &'static str,
    pub cancel: &'static str,
    pub submit: &'static str,
    pub submitting: &'static str,
    pub hint_cancel: &'static str,
    pub hint_submit: &'static str,

    pub toast_brief_ready: &'static str,
    pub toast_review_done: &'static str,
    pub toast_review_conflict: &'static str,
    pub toast_review_failed: &'static str,
    pub toast_audit_failed: &'static str,
    pub toast_exported: &'static str,
    pub toast_export_cancelled: &'static str,
    pub toast_degraded_render: &'static str,
    pub queue_unavailable: &'static str,
    pub queue_empty: &'static str,
}

pub const ZH: Strings = Strings {
    app_title: "WB 财报审核工作台",
    refresh_queue: "刷新队列",
    service_ok: "研究服务已连接",
    service_down: "服务不可用：",
    connecting: "连接中…",
    refreshed_at: " · 刷新 ",

    nav_inbox: "审核收件箱",
    nav_library: "简报库",
    nav_quadrant: "事件四象限",
    nav_audit: "审核台账",
    col_priority: "优先级",
    col_company: "公司",
    col_period: "期间",
    col_status: "状态",
    col_time: "更新时间",
    col_task: "任务 ID",
    meta_task: "任务",
    meta_trace: "trace",
    meta_object: "对象",
    meta_workflow: "工作流",
    shortcuts_title: "键盘快捷键",
    sc_inbox: "审核收件箱",
    sc_library: "简报库",
    sc_quadrant: "事件四象限",
    sc_audit: "审核台账",
    sc_refresh: "刷新队列",
    sc_back: "返回 / 关闭",
    sc_submit: "提交审核（评审弹窗中）",
    close: "关闭",
    expand_full: "展开完整视图",
    meta_toggle: "系统与审计元数据",
    reader_open_review: "在审核视图中打开",
    reader_reviewer: "审核人",
    q_axis_urgent: "紧急",
    q_axis_not_urgent: "不紧急",
    q_axis_important: "重要",
    q_axis_minor: "次要",
    q_empty_p1: "无紧急事件，全部就绪",
    q_empty: "此象限暂无事件",
    q_review: "审核",

    inbox_title: "审核收件箱（F5）",
    inbox_hint: "按优先级排列；点击任务在右侧预览并审核",
    all: "全部",
    company_filter_placeholder: "按公司/代码过滤…",
    loading_queue: "加载队列…",
    queue_degraded: "研究服务不可用（队列可能不是最新）：",

    library_title: "研究简报库（F1 · I2 vault）",
    library_search_placeholder: "全文搜索简报（标题/正文/数字）…",
    library_empty: "vault 中没有简报（启动 wb-mock 后会自动写入样例）。",

    quadrant_title: "事件四象限（F12 · 重要×紧急）",
    q_p1: "P1 紧急 × 重要",
    q_p2: "P2 重要 × 非紧急",
    q_p3: "P3 紧急 × 次要",
    q_p4: "P4 非紧急 × 次要",

    audit_export: "导出 JSON",
    audit_hint: "每次审核本地先落盘（服务中断不丢）；回执来自 I3 审计链。",
    audit_empty: "暂无审核记录 — 到收件箱完成一次审核后回看。",

    back: "返回列表",
    brief_suffix: " 财报研究简报",
    cutoff_stale: "（超过 3 天）",
    detail_loading: "加载任务详情…",
    detail_failed: "详情加载失败：",
    not_reviewable: "当前状态 {}，不在待审核队列；操作历史见下方状态机记录",
    approve: "通过",
    approve_as_revised: "修改后通过",
    reject: "驳回",
    review_via_mcp: "审核将经由 MCP 通道（I3）提交并写入审计链",
    evidence_toggle: "证据下钻",
    evidence_hide: "收起",
    evidence_sub: "公告摘录 / 指标 / 证据包 / 口径与版本",
    evidence_title: "证据下钻（F11）",
    evidence_excerpt: "excerpt（公告摘录）",
    evidence_announcement: "公告（ContextBundle 快照）",
    evidence_methodology: "methodology（口径与版本）",
    history_title: "过程时间线",

    sec_fact: "事实 FACT",
    sec_calc: "计算 CALCULATION（确定性工具产出）",
    sec_inference: "推断 INFERENCE（模型生成）",
    sec_check: "校验 CHECK",
    guard_ok: "数字校验通过：推断节数字全部命中证据数字（容差 1%）",
    guard_fail: "数字校验未通过 {} 项：{}",

    review_title: "提交审核（F5/F6）",
    comment_label: "审核意见（可选）",
    comment_placeholder: "",
    revise_label: "修改稿（编辑推断节；提交后原稿保留、版本升 v2）",
    revise_placeholder: "修改推断节…",
    diff_same: "与原稿一致（尚未修改）",
    diff_changed: "与原稿的差异：{} 处变更（实时 diff，原文不动）",
    revise_empty: "修改稿不能为空：请编辑推断节后再提交",
    cancel: "取消",
    submit: "提交审核",
    submitting: "提交中…",
    hint_cancel: "取消",
    hint_submit: "提交",

    toast_brief_ready: "新简报待审核：",
    toast_review_done: "审核完成：",
    toast_review_conflict: "审核冲突：任务已不在待审核（",
    toast_review_failed: "审核提交失败：",
    toast_audit_failed: "审计日志写入失败：",
    toast_exported: "已导出 {} 条审核记录 → {}",
    toast_export_cancelled: "已取消导出",
    toast_degraded_render: "研究服务不可用，已从简报库渲染（",
    queue_unavailable: "队列不可用：显示为空而非误判为“无待审”。可稍后刷新。",
    queue_empty: "没有符合条件的任务。",
};

pub const EN: Strings = Strings {
    app_title: "WB Financial Review Workbench",
    refresh_queue: "Refresh",
    service_ok: "Research service connected",
    service_down: "Service unavailable: ",
    connecting: "Connecting…",
    refreshed_at: " · refreshed ",

    nav_inbox: "Review inbox",
    nav_library: "Brief library",
    nav_quadrant: "Event quadrant",
    nav_audit: "Audit trail",
    col_priority: "Priority",
    col_company: "Company",
    col_period: "Period",
    col_status: "Status",
    col_time: "Updated",
    col_task: "Task ID",
    meta_task: "Task",
    meta_trace: "trace",
    meta_object: "Object",
    meta_workflow: "Workflow",
    shortcuts_title: "Keyboard shortcuts",
    sc_inbox: "Review inbox",
    sc_library: "Brief library",
    sc_quadrant: "Event quadrant",
    sc_audit: "Audit trail",
    sc_refresh: "Refresh queue",
    sc_back: "Back / close",
    sc_submit: "Submit review (in review dialog)",
    close: "Close",
    expand_full: "Open full view",
    meta_toggle: "System & audit metadata",
    reader_open_review: "Open in review view",
    reader_reviewer: "Reviewer",
    q_axis_urgent: "Urgent",
    q_axis_not_urgent: "Not urgent",
    q_axis_important: "Important",
    q_axis_minor: "Minor",
    q_empty_p1: "No urgent tasks — all caught up",
    q_empty: "No events in this quadrant",
    q_review: "Review",

    inbox_title: "Review inbox (F5)",
    inbox_hint: "Sorted by priority; click a task to preview and review it on the right",
    all: "All",
    company_filter_placeholder: "Filter by company / code…",
    loading_queue: "Loading queue…",
    queue_degraded: "Research service unavailable (queue may be stale): ",

    library_title: "Brief library (F1 · I2 vault)",
    library_search_placeholder: "Full-text search (title / body / numbers)…",
    library_empty: "No briefs in the vault yet (wb-mock seeds samples on startup).",

    quadrant_title: "Event quadrant (F12 · importance × urgency)",
    q_p1: "P1 urgent × important",
    q_p2: "P2 important × not urgent",
    q_p3: "P3 urgent × minor",
    q_p4: "P4 not urgent × minor",

    audit_export: "Export JSON",
    audit_hint: "Every review is written locally first (survives outages); receipts come from the I3 audit chain.",
    audit_empty: "No reviews yet — approve something in the inbox and come back.",

    back: "Back to list",
    brief_suffix: " earnings research brief",
    cutoff_stale: " (older than 3 days)",
    detail_loading: "Loading task detail…",
    detail_failed: "Failed to load detail: ",
    not_reviewable: "Status is {}; not in the review queue — see the state-machine history below",
    approve: "Approve",
    approve_as_revised: "Approve as revised",
    reject: "Reject",
    review_via_mcp: "Reviews go through the MCP channel (I3) and write the audit chain",
    evidence_toggle: "Evidence",
    evidence_hide: "Collapse",
    evidence_sub: "announcement excerpt / metrics / evidence pack / methodology",
    evidence_title: "Evidence drill-down (F11)",
    evidence_excerpt: "excerpt (announcement)",
    evidence_announcement: "Announcement (ContextBundle snapshot)",
    evidence_methodology: "methodology (calibration & versions)",
    history_title: "Process timeline",

    sec_fact: "FACT",
    sec_calc: "CALCULATION (deterministic tool output)",
    sec_inference: "INFERENCE (model-generated)",
    sec_check: "CHECK",
    guard_ok: "Numeric check passed: every number in INFERENCE traces to evidence (1% tolerance)",
    guard_fail: "Numeric check failed on {} value(s): {}",

    review_title: "Submit review (F5/F6)",
    comment_label: "Comment (optional)",
    comment_placeholder: "",
    revise_label: "Revised draft (edit INFERENCE; original kept, version bumps to v2)",
    revise_placeholder: "Edit INFERENCE…",
    diff_same: "Unchanged vs original (no edits yet)",
    diff_changed: "Diff vs original: {} change(s) (live; original untouched)",
    revise_empty: "Revised draft is empty: edit the INFERENCE section before submitting",
    cancel: "Cancel",
    submit: "Submit review",
    submitting: "Submitting…",
    hint_cancel: "Cancel",
    hint_submit: "Submit",

    toast_brief_ready: "New brief pending review: ",
    toast_review_done: "Review done: ",
    toast_review_conflict: "Review conflict: task left the review queue (",
    toast_review_failed: "Review submit failed: ",
    toast_audit_failed: "Failed to write audit log: ",
    toast_exported: "Exported {} audit record(s) → {}",
    toast_export_cancelled: "Export cancelled",
    toast_degraded_render: "Research service unavailable — rendered from the vault (",
    queue_unavailable: "Queue unavailable: showing empty rather than a false “nothing pending”. Refresh later.",
    queue_empty: "No tasks match the current filter.",
};

pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Zh => &ZH,
        Lang::En => &EN,
    }
}

/// Status badges accept both contract states (WAITING_REVIEW…) and the I2
/// frontmatter's Chinese labels (待审核…). Unknown statuses pass through
/// unchanged as an owned value (no allocation for the known ones).
pub fn status_label(lang: Lang, status: &str) -> Cow<'static, str> {
    match (lang, status) {
        (Lang::En, "WAITING_REVIEW" | "待审核") => "Waiting review".into(),
        (Lang::En, "SUCCEEDED" | "已通过") => "Approved".into(),
        (Lang::En, "FAILED" | "已失败") => "Failed".into(),
        (Lang::En, "CANCELLED" | "已驳回") => "Rejected".into(),
        (Lang::En, "RUNNING") => "Running".into(),
        (Lang::En, "CONTEXT_READY") => "Context ready".into(),
        (Lang::En, "TASK_CREATED") => "Task created".into(),
        (Lang::En, "RECEIVED") => "Received".into(),
        (Lang::En, "IGNORED") => "Ignored".into(),
        (Lang::En, "WAITING_DATA") => "Waiting data".into(),
        (Lang::Zh, "WAITING_REVIEW" | "待审核") => "待审核".into(),
        (Lang::Zh, "SUCCEEDED" | "已通过") => "已通过".into(),
        (Lang::Zh, "FAILED" | "已失败") => "失败".into(),
        (Lang::Zh, "CANCELLED" | "已驳回") => "已驳回".into(),
        (Lang::Zh, "RUNNING" | "CONTEXT_READY" | "TASK_CREATED") => "进行中".into(),
        (Lang::Zh, "RECEIVED") => "已接收".into(),
        (Lang::Zh, "IGNORED" | "WAITING_DATA") => "已忽略".into(),
        (_, other) => Cow::Owned(other.to_string()),
    }
}

/// Verdict labels for audit records (stored as the contract's Chinese values).
pub fn verdict_label(lang: Lang, verdict: &str) -> Cow<'static, str> {
    match (lang, verdict) {
        (Lang::En, "通过") => "Approve".into(),
        (Lang::En, "驳回") => "Reject".into(),
        (Lang::En, "修改后通过") => "Approve as revised".into(),
        (_, other) => Cow::Owned(other.to_string()),
    }
}

/// English rendition of the guardrail violation hint (the core builds the Chinese one).
pub fn guardrail_hint_en(raw: &str) -> String {
    format!(
        "「{raw}」does not trace to any evidence number (tolerance 1% / 0.02pp). Model-computed differences are not evidence — quote the announcement's own numbers instead."
    )
}
