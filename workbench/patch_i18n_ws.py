GLYPH_INBOX = chr(92) + 'u{E715}'
GLYPH_LIB = chr(92) + 'u{E8F1}'
GLYPH_QUAD = chr(92) + 'u{E9D9}'
GLYPH_HIST = chr(92) + 'u{E81C}'
GLYPH_REFRESH = chr(92) + 'u{E72C}'
GLYPH_BACK = chr(92) + 'u{E72B}'
GLYPH_EXPORT = chr(92) + 'u{E898}'
GLYPH_COLLAPSE = chr(92) + 'u{E738}'

# =====================================================================
# workspace.rs
# =====================================================================
p = 'crates/wb-app/src/workspace.rs'
s = open(p, encoding='utf-8').read()

s = s.replace('use crate::theme::{sid, *};',
              'use crate::i18n::{self, Lang};\nuse crate::theme::{sid, *};')

s = s.replace('''pub struct Workspace {
    pub cfg: AppConfig,''', '''pub struct Workspace {
    pub cfg: AppConfig,
    pub lang: Lang,''')

s = s.replace('''        let cfg = AppConfig::load();''', '''        let cfg = AppConfig::load();
        let lang = Lang::from_config(&cfg.lang);''')
s = s.replace('''        let mut ws = Workspace {
            cfg,''', '''        let mut ws = Workspace {
            cfg,
            lang,''')

# ---------------- topbar ----------------
old = '''        let (dot, label) = match self.service_ok {
            Some(true) => ("●", "研究服务已连接".to_string()),
            Some(false) => ("●", format!("服务不可用：{}", self.queue_error.clone().unwrap_or_default())),
            None => ("○", "连接中…".to_string()),
        };'''
new = '''        let ts = i18n::strings(self.lang);
        let (dot, label) = match self.service_ok {
            Some(true) => ("●", ts.service_ok.to_string()),
            Some(false) => ("●", format!("{}{}", ts.service_down, self.queue_error.clone().unwrap_or_default())),
            None => ("○", ts.connecting.to_string()),
        };'''
assert old in s, "topbar status"
s = s.replace(old, new)

old = '.child("WB 财报审核工作台"),'
new = '.child(ts.app_title),'
assert old in s, "topbar title"
s = s.replace(old, new)

old = '''            .child(
                self.button("\\u{E72C} 刷新队列", BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {
                    ws.reload_queue(cx);
                }),
            )'''
new = '''            .child(
                self.button(self.lang.toggle_label(), BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {
                    ws.toggle_language(cx);
                }),
            )
            .child(
                self.button(ts.refresh_queue, BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {
                    ws.reload_queue(cx);
                }),
            )'''
assert old in s, "topbar buttons"
s = s.replace(old, new)

old = '''                        self.last_refresh
                            .as_ref()
                            .map(|t| format!(" · 刷新 {t}"))
                            .unwrap_or_default()'''
new = '''                        self.last_refresh
                            .as_ref()
                            .map(|t| format!("{}{t}", ts.refreshed_at))
                            .unwrap_or_default()'''
assert old in s, "refreshed at"
s = s.replace(old, new)

# toggle_language
old = '''    // ------------------------------------------------------------- data flow

    pub fn reload_queue(&mut self, cx: &mut Context<Self>) {'''
new = '''    // ------------------------------------------------------------- data flow

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

    pub fn reload_queue(&mut self, cx: &mut Context<Self>) {'''
assert old in s, "toggle anchor"
s = s.replace(old, new)

# ---------------- sidebar ----------------
old = '''        let items = [
            (ViewKind::Inbox, "''' + GLYPH_INBOX + '''", "审核收件箱", format!("{waiting} 待审 · {failed} 失败")),
            (ViewKind::Library, "''' + GLYPH_LIB + '''", "简报库", format!("{} 篇", self.library_hits.len())),
            (ViewKind::Quadrant, "''' + GLYPH_QUAD + '''", "事件四象限", "P1–P4".to_string()),
            (ViewKind::AuditTrail, "''' + GLYPH_HIST + '''", "审核台账", "本地留存".to_string()),
        ];'''
new = '''        let ts = i18n::strings(self.lang);
        let items = [
            (ViewKind::Inbox, "''' + GLYPH_INBOX + '''", ts.nav_inbox, match self.lang {
                Lang::Zh => format!("{waiting} 待审 · {failed} 失败"),
                Lang::En => format!("{waiting} waiting · {failed} failed"),
            }),
            (ViewKind::Library, "''' + GLYPH_LIB + '''", ts.nav_library, match self.lang {
                Lang::Zh => format!("{} 篇", self.library_hits.len()),
                Lang::En => format!("{} briefs", self.library_hits.len()),
            }),
            (ViewKind::Quadrant, "''' + GLYPH_QUAD + '''", ts.nav_quadrant, "P1–P4".to_string()),
            (ViewKind::AuditTrail, "''' + GLYPH_HIST + '''", ts.nav_audit, match self.lang {
                Lang::Zh => "本地留存".to_string(),
                Lang::En => "local-first".to_string(),
            }),
        ];'''
assert old in s, "nav items"
s = s.replace(old, new)

old = '.child(label.to_string()),'
new = '.child(label),'
assert old in s, "nav label"
s = s.replace(old, new)

old = '''                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child("F4 护栏：推断节数字须命中证据"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child("F7 台账：本地 JSONL，服务中断可写"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child("Ctrl+1–4 切换 · Ctrl+R 刷新 · Esc 返回"),
                ),'''
new = '''                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child(match self.lang {
                            Lang::Zh => "F4 护栏：推断节数字须命中证据",
                            Lang::En => "F4 guardrail: INFERENCE numbers must trace to evidence",
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child(match self.lang {
                            Lang::Zh => "F7 台账：本地 JSONL，服务中断可写",
                            Lang::En => "F7 audit: local JSONL, writable during outages",
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child(ts.nav_shortcuts),
                ),'''
assert old in s, "legend"
s = s.replace(old, new)

# ---------------- inbox ----------------
old = '''    fn render_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let chips: [Option<&str>; 5] = ['''
new = '''    fn render_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let chips: [Option<&str>; 5] = ['''
assert old in s, "inbox head"
s = s.replace(old, new)

old = '.child("状态："),'
new = '.child(ts.status_filter),'
assert old in s, "status label"
s = s.replace(old, new)

old = '''            let label = match chip {
                None => format!("全部 {count}"),
                Some(cs) => format!("{} {count}", status_label(cs)),
            };'''
new = '''            let label = match chip {
                None => format!("{} {count}", ts.all),
                Some(cs) => format!("{} {count}", i18n::status_label(self.lang, cs)),
            };'''
assert old in s, "chip labels"
s = s.replace(old, new)

old = '''.child("审核收件箱（F5）"),'''
new = '''.child(ts.inbox_title),'''
assert old in s, "inbox title"
s = s.replace(old, new)

old = '''.child("状态机：RECEIVED → … → WAITING_REVIEW → SUCCEEDED；异常态仅可查看"),'''
new = '''.child(ts.inbox_hint),'''
assert old in s, "inbox hint"
s = s.replace(old, new)

old = '.child(format!("研究服务不可用（队列可能不是最新）：{err}")),'
new = '.child(format!("{}{err}", ts.queue_degraded)),'
assert old in s, "degraded"
s = s.replace(old, new)

old = 'pane = pane.child(div().p_3().text_color(c(DIM)).child("加载队列…"));'
new = 'pane = pane.child(div().p_3().text_color(c(DIM)).child(ts.loading_queue));'
assert old in s, "loading"
s = s.replace(old, new)

old = '''                    .child(if self.queue_error.is_some() {
                        "队列不可用：显示为空而非误判为“无待审”。可稍后刷新。".to_string()
                    } else {
                        "没有符合条件的任务。".to_string()
                    }),'''
new = '''                    .child(if self.queue_error.is_some() {
                        ts.queue_unavailable.to_string()
                    } else {
                        ts.queue_empty.to_string()
                    }),'''
assert old in s, "inbox empty"
s = s.replace(old, new)

old = '''                .child(Badge::new(
                    status_label(&t.status),
                    status_tone(&t.status),
                ))'''
new = '''                .child(Badge::new(
                    i18n::status_label(self.lang, &t.status),
                    status_tone(&t.status),
                ))'''
assert old in s, "row badge"
s = s.replace(old, new)

# ---------------- library ----------------
old = '''    fn render_library(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let mut pane = div().flex().flex_col().flex_1().min_h_0().gap_2();
        pane = pane
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("研究简报库（F1 · I2 vault）"),
            )'''
new = '''    fn render_library(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut pane = div().flex().flex_col().flex_1().min_h_0().gap_2();
        pane = pane
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(ts.library_title),
            )'''
assert old in s, "library head"
s = s.replace(old, new)

old = '''                    .child(format!(
                        "vault：{}（简报全文索引；点击查看四节渲染）",
                        self.cfg.vault_root.display()
                    )),'''
new = '''                    .child(format!(
                        "vault：{} {}",
                        self.cfg.vault_root.display(),
                        ts.library_vault_hint
                    )),'''
assert old in s, "library hint"
s = s.replace(old, new)

old = '''            list = list.child(
                div()
                    .p_6()
                    .text_color(c(DIM))
                    .child("vault 中没有简报（启动 wb-mock 后会自动写入样例）。"),
            );'''
new = '''            list = list.child(
                div().p_6().text_color(c(DIM)).child(ts.library_empty),
            );'''
assert old in s, "library empty"
s = s.replace(old, new)

old = '''                        .child(Badge::new(
                            format!("{status} · v1"),
                            status_tone(&status),
                        )),'''
new = '''                        .child(Badge::new(
                            format!("{} · v1", i18n::status_label(self.lang, &status)),
                            status_tone(&status),
                        )),'''
assert old in s, "library badge"
s = s.replace(old, new)

old = '.child(format!("{tid}（score {score}）")),'
new = '''.child(format!(
                            "{tid}{}{})",
                            ts.library_score, score
                        )),'''
assert old in s, "library score"
s = s.replace(old, new)

# ---------------- quadrant ----------------
old = '''    fn render_quadrant(&mut self, w: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let now = chrono::Local::now().naive_local();'''
new = '''    fn render_quadrant(&mut self, w: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let now = chrono::Local::now().naive_local();'''
assert old in s, "quadrant head"
s = s.replace(old, new)

old = '''        let titles = [
            ("P1 紧急 × 重要", BadgeTone::Red),
            ("P2 重要 × 非紧急", BadgeTone::Amber),
            ("P3 紧急 × 次要", BadgeTone::Blue),
            ("P4 非紧急 × 次要", BadgeTone::Neutral),
        ];'''
new = '''        let titles = [
            (ts.q_p1, BadgeTone::Red),
            (ts.q_p2, BadgeTone::Amber),
            (ts.q_p3, BadgeTone::Blue),
            (ts.q_p4, BadgeTone::Neutral),
        ];'''
assert old in s, "quadrant titles"
s = s.replace(old, new)

old = '''.child(format!("{label}（{}）", cells[idx].len())),'''
new = '''.child(match self.lang {
                            Lang::Zh => format!("{label}（{}）", cells[idx].len()),
                            Lang::En => format!("{} ({})", label, cells[idx].len()),
                        }),'''
assert old in s, "quadrant count"
s = s.replace(old, new)

old = '''.child("事件四象限（F12 · 重要×紧急）"),'''
new = '''.child(ts.quadrant_title),'''
assert old in s, "quadrant title"
s = s.replace(old, new)

old = '.child(Badge::new(status_label(&t.status), status_tone(&t.status)))'
new = '''.child(Badge::new(
                                i18n::status_label(self.lang, &t.status),
                                status_tone(&t.status),
                            ))'''
assert old in s, "quadrant badge"
s = s.replace(old, new)

# ---------------- audit ----------------
old = '''    fn render_audit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let records = self.audit.list();'''
new = '''    fn render_audit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let records = self.audit.list();'''
assert old in s, "audit head"
s = s.replace(old, new)

old = '''.child(format!("审核台账（F7 · 本地留存 {} 条）", records.len())),'''
new = '''.child(match self.lang {
                                Lang::Zh => format!("审核台账（F7 · 本地留存 {} 条）", records.len()),
                                Lang::En => format!("Audit trail (F7 · local, {})", records.len()),
                            }),'''
assert old in s, "audit title"
s = s.replace(old, new)

old = '''                        self.button(
                            "''' + GLYPH_EXPORT + ''' 导出 JSON",'''
new = '''                        self.button(
                            ts.audit_export,'''
assert old in s, "audit export"
s = s.replace(old, new)

old = '''.child("每次审核本地先落盘（服务中断不丢）；回执来自 I3 审计链。"),'''
new = '''.child(ts.audit_hint),'''
assert old in s, "audit hint"
s = s.replace(old, new)

old = '''                    .child("暂无审核记录 — 到收件箱完成一次审核后回看。"),'''
new = '''                    .child(ts.audit_empty),'''
assert old in s, "audit empty"
s = s.replace(old, new)

old = '.child(Badge::new(r.verdict.clone(), tone))'
new = '''.child(Badge::new(
                                i18n::verdict_label(self.lang, &r.verdict),
                                tone,
                            ))'''
assert old in s, "audit verdict"
s = s.replace(old, new)

# ---------------- toasts ----------------
old = '''                for ev in &new_events {
                    ws.push_toast(
                        format!("新简报待审核：{}", ev.title),
                        ToastKind::Info,
                        cx,
                    );
                }'''
new = '''                for ev in &new_events {
                    let ts = i18n::strings(ws.lang);
                    ws.push_toast(
                        format!("{}{}", ts.toast_brief_ready, ev.title),
                        ToastKind::Info,
                        cx,
                    );
                }'''
assert old in s, "webhook toast"
s = s.replace(old, new)

old = '''                Ok(receipt) => {
                    let object_name = ws'''
new = '''                Ok(receipt) => {
                    let ts = i18n::strings(ws.lang);
                    let object_name = ws'''
assert old in s, "submit ok head"
s = s.replace(old, new)

old = '''                    if let Err(e) = ws.audit.record_review(&req_for_audit, &object_name, &period, &receipt) {
                        ws.push_toast(format!("审计日志写入失败：{e}"), ToastKind::Error, cx);
                    }'''
new = '''                    if let Err(e) = ws.audit.record_review(&req_for_audit, &object_name, &period, &receipt) {
                        ws.push_toast(
                            format!("{}{e}", ts.toast_audit_failed),
                            ToastKind::Error,
                            cx,
                        );
                    }'''
assert old in s, "audit failed toast"
s = s.replace(old, new)

old = '''                    let msg = match receipt.brief_version {
                        Some(v) => format!(
                            "审核完成：{} → {}（简报 v{v}，{}）",
                            verdict.as_str(),
                            receipt.status,
                            receipt.review_id
                        ),
                        None => format!(
                            "审核完成：{} → {}（{}）",
                            verdict.as_str(),
                            receipt.status,
                            receipt.review_id
                        ),
                    };'''
new = '''                    let verdict_label = i18n::verdict_label(ws.lang, verdict.as_str());
                    let msg = match (ws.lang, receipt.brief_version) {
                        (Lang::Zh, Some(v)) => format!(
                            "{}{} → {}（简报 v{v}，{}）",
                            ts.toast_review_done,
                            verdict_label,
                            receipt.status,
                            receipt.review_id
                        ),
                        (Lang::En, Some(v)) => format!(
                            "{}{} → {} (brief v{v}, {})",
                            ts.toast_review_done,
                            verdict_label,
                            receipt.status,
                            receipt.review_id
                        ),
                        (_, _) => format!(
                            "{}{} → {}",
                            ts.toast_review_done,
                            verdict_label,
                            receipt.review_id
                        ),
                    };'''
assert old in s, "done toast"
s = s.replace(old, new)

old = '''                    ws.push_toast(
                        format!("审核冲突：任务已不在待审核（{current_status}）"),
                        ToastKind::Warn,
                        cx,
                    );'''
new = '''                    let ts = i18n::strings(ws.lang);
                    ws.push_toast(
                        format!("{}{current_status})", ts.toast_review_conflict),
                        ToastKind::Warn,
                        cx,
                    );'''
assert old in s, "conflict toast"
s = s.replace(old, new)

old = 'ws.push_toast(format!("审核提交失败：{e}"), ToastKind::Error, cx);'
new = '''let ts = i18n::strings(ws.lang);
                    ws.push_toast(format!("{}{e}", ts.toast_review_failed), ToastKind::Error, cx);'''
assert old in s, "failed toast"
s = s.replace(old, new)

old = '''            this.update(cx, |ws, cx| match picked {
                Some(path) => match audit.export_json(&path) {
                    Ok(n) => ws.push_toast(
                        format!("已导出 {n} 条审核记录 → {}", path.display()),
                        ToastKind::Success,
                        cx,
                    ),
                    Err(e) => ws.push_toast(format!("导出失败：{e}"), ToastKind::Error, cx),
                },
                None => ws.push_toast("已取消导出".to_string(), ToastKind::Info, cx),
            })
            .ok();'''
new = '''            this.update(cx, |ws, cx| {
                let ts = i18n::strings(ws.lang);
                match picked {
                    Some(path) => match audit.export_json(&path) {
                        Ok(n) => ws.push_toast(
                            match ws.lang {
                                Lang::Zh => format!("已导出 {n} 条审核记录 → {}", path.display()),
                                Lang::En => format!("Exported {n} audit record(s) → {}", path.display()),
                            },
                            ToastKind::Success,
                            cx,
                        ),
                        Err(e) => ws.push_toast(
                            format!("{}{e}", ts.toast_review_failed),
                            ToastKind::Error,
                            cx,
                        ),
                    },
                    None => ws.push_toast(
                        ts.toast_export_cancelled.to_string(),
                        ToastKind::Info,
                        cx,
                    ),
                }
            })
            .ok();'''
assert old in s, "export toasts"
s = s.replace(old, new)

old = 'ws.detail_error = Some(format!("研究服务不可用，已从简报库渲染（{e}）"));'
new = '''let ts = i18n::strings(ws.lang);
                                ws.detail_error = Some(format!("{}{e}）", ts.toast_degraded_render));'''
assert old in s, "degraded render"
s = s.replace(old, new)

old = '''            if let Some(flow) = &mut self.review {
                flow.error = Some("修改稿不能为空：请编辑推断节后再提交".into());
            }'''
new = '''            if let Some(flow) = &mut self.review {
                flow.error = Some(i18n::strings(self.lang).revise_empty.into());
            }'''
assert old in s, "empty revise"
s = s.replace(old, new)

# review modal comment input placeholder at creation
old = 'let comment = cx.new(|cx| TextInput::new(false, "审核意见（可选）…", false, cx));'
new = 'let comment = cx.new(|cx| {\n            TextInput::new(false, i18n::strings(self.lang).comment_placeholder, false, cx)\n        });'
assert old in s, "comment input"
s = s.replace(old, new)

open(p, 'w', encoding='utf-8').write(s)
print("workspace i18n applied")
