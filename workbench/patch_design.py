import re

# ============ theme.rs: icon helper + button tokens ============
p = 'crates/wb-app/src/theme.rs'
s = open(p, encoding='utf-8').read()

s = s.replace('''pub const FONT_UI: &str = "Segoe UI";
pub const FONT_MONO: &str = "Consolas";''',
'''pub const FONT_UI: &str = "Segoe UI";
pub const FONT_MONO: &str = "Consolas";
/// Segoe Fluent Icons (Windows 11 system font) for small UI glyphs.
pub const FONT_ICON: &str = "Segoe Fluent Icons";

/// A small monochrome glyph from Segoe Fluent Icons.
pub fn icon(code: &str, size: f32, color: Hsla) -> gpui::Div {
    gpui::div()
        .font_family(FONT_ICON)
        .text_size(px(size))
        .text_color(color)
        .child(code.to_string())
}''')

s = s.replace('''pub const RED_BG: u32 = 0x3a2020;''',
'''pub const BTN: u32 = 0x262b36;
pub const BTN_HOVER: u32 = 0x303645;
pub const RED_BG: u32 = 0x3a2020;''')

open(p, 'w', encoding='utf-8').write(s)

# ============ workspace.rs ============
p = 'crates/wb-app/src/workspace.rs'
s = open(p, encoding='utf-8').read()

# --- 1. buttons: borderless filled, real hover
old = '''        let listener = cx.listener(handler);
        let mut el = div()
            .id(sid(format!("btn-{label}")))
            .px_3()
            .py_1()
            .rounded_sm()
            .text_size(px(12.5))
            .cursor_pointer();
        el = if filled {
            el.bg(tone.bg()).text_color(tone.fg())
        } else {
            el.bg(c(PANEL2)).text_color(c(FG))
        };
        el.border_1()
            .border_color(tone.fg())
            .hover(|s| s.bg(tone.bg()).text_color(tone.fg()))
            .child(label.to_string())
            .on_click(move |e, w, cx| listener(e, w, cx))'''
new = '''        let listener = cx.listener(handler);
        let mut el = div()
            .id(sid(format!("btn-{label}")))
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .px_3()
            .py_1()
            .rounded_sm()
            .text_size(px(12.0));
        el = if filled {
            el.bg(tone.bg()).text_color(tone.fg())
                .hover(|s| s.bg(c(BTN_HOVER)))
        } else {
            el.bg(c(BTN)).text_color(c(FG))
                .hover(|s| s.bg(c(BTN_HOVER)))
        };
        el.child(label.to_string())
            .on_click(move |e, w, cx| listener(e, w, cx))'''
assert old in s, "button not found"
s = s.replace(old, new)

# --- 2. nav items with icons
old = '''        let items = [
            (ViewKind::Inbox, "\u5ba1\u6838\u6536\u4ef6\u7bb1", format!("{waiting} \u5f85\u5ba1 \u00b7 {failed} \u5931\u8d25")),
            (ViewKind::Library, "\u7b80\u62a5\u5e93", format!("{} \u7bc7", self.library_hits.len())),
            (ViewKind::Quadrant, "\u4e8b\u4ef6\u56db\u8c61\u9650", "P1\u2013P4".to_string()),
            (ViewKind::AuditTrail, "\u5ba1\u6838\u53f0\u8d26", "\u672c\u5730\u7559\u5b58".to_string()),
        ];'''
new = '''        let items = [
            (ViewKind::Inbox, "\\u{E715}", "\u5ba1\u6838\u6536\u4ef6\u7bb1", format!("{waiting} \u5f85\u5ba1 \u00b7 {failed} \u5931\u8d25")),
            (ViewKind::Library, "\\u{E8F1}", "\u7b80\u62a5\u5e93", format!("{} \u7bc7", self.library_hits.len())),
            (ViewKind::Quadrant, "\\u{E9D9}", "\u4e8b\u4ef6\u56db\u8c61\u9650", "P1\u2013P4".to_string()),
            (ViewKind::AuditTrail, "\\u{E81C}", "\u5ba1\u6838\u53f0\u8d26", "\u672c\u5730\u7559\u5b58".to_string()),
        ];'''
assert old in s, "nav items not found"
s = s.replace(old, new)

old = '''        for (kind, label, sub) in items {
            let active = self.view == kind;
            let mut item = div()
                .id(sid(format!("nav-{}", kind.label())))
                .flex()
                .flex_col()
                .px_2()
                .py_1p5()
                .rounded_sm()
                .cursor_pointer();
            item = if active {
                item.bg(c(PANEL2)).border_l_2().border_color(c(ACCENT))
            } else {
                item.hover(|s| s.bg(c(PANEL2)))
            };
            sidebar = sidebar.child(
                item.child(
                    div()
                        .text_size(px(13.0))
                        .text_color(if active { c(FG) } else { c(DIM) })
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(c(DIM))
                        .child(sub),
                )
                .on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                    ws.view = kind;
                    ws.hovered_violation = None;
                    cx.notify();
                })),
            );
        }'''
new = '''        for (kind, glyph, label, sub) in items {
            let active = self.view == kind;
            let mut item = div()
                .id(sid(format!("nav-{}", kind.label())))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1p5()
                .rounded_sm();
            item = if active {
                item.bg(c(PANEL2)).border_l_2().border_color(c(ACCENT))
            } else {
                item.hover(|s| s.bg(c(PANEL2)))
            };
            sidebar = sidebar.child(
                item.child(icon(glyph, 14.0, if active { c(ACCENT) } else { c(DIM) }))
                    .child(
                        div().flex().flex_col()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(if active { c(FG) } else { c(DIM) })
                                    .child(label.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(c(DIM))
                                    .child(sub),
                            ),
                    )
                    .on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                        ws.view = kind;
                        ws.hovered_violation = None;
                        cx.notify();
                    })),
            );
        }'''
assert old in s, "nav item loop not found"
s = s.replace(old, new)

# --- 3. inbox rows: borderless, task_id instead of redundant title
old = '''            let mut row = div()
                .id(sid(format!("row-{tid}")))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded_sm()
                .bg(c(PANEL))
                .border_1()
                .border_color(c(BORDER));
            row = row.hover(|s| s.bg(c(PANEL2)).border_color(c(ACCENT)));
            let row = row
                .child(Badge::new(t.priority.clone(), priority_tone(&t.priority)))
                .child(
                    div()
                        .w(px(90.0))
                        .text_size(px(13.0))
                        .text_color(c(FG))
                        .child(t.object_name.clone()),
                )
                .child(
                    div()
                        .w(px(70.0))
                        .text_size(px(12.0))
                        .text_color(c(DIM))
                        .font_family(FONT_MONO)
                        .child(t.period.clone()),
                )
                .child(
                    div()
                        .w(px(380.0))
                        .min_w_0()
                        .text_size(px(12.5))
                        .text_color(c(DIM))
                        .truncate()
                        .child(t.title.clone()),
                )
                .child(Badge::new(
                    status_label(&t.status),
                    status_tone(&t.status),
                ))
                .child(
                    div()
                        .w(px(150.0))
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .font_family(FONT_MONO)
                        .child(t.updated_at.clone()),
                )
                .child(div().flex_1());'''
new = '''            let mut row = div()
                .id(sid(format!("row-{tid}")))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded_sm()
                .bg(c(PANEL));
            row = row.hover(|s| s.bg(c(PANEL2)));
            let row = row
                .child(Badge::new(t.priority.clone(), priority_tone(&t.priority)))
                .child(
                    div()
                        .w(px(90.0))
                        .text_size(px(13.0))
                        .text_color(c(FG))
                        .child(t.object_name.clone()),
                )
                .child(
                    div()
                        .w(px(70.0))
                        .text_size(px(12.0))
                        .text_color(c(DIM))
                        .font_family(FONT_MONO)
                        .child(t.period.clone()),
                )
                .child(
                    div()
                        .w(px(380.0))
                        .min_w_0()
                        .text_size(px(12.0))
                        .text_color(c(DIM))
                        .font_family(FONT_MONO)
                        .truncate()
                        .child(t.task_id.clone()),
                )
                .child(Badge::new(
                    status_label(&t.status),
                    status_tone(&t.status),
                ))
                .child(
                    div()
                        .w(px(150.0))
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .font_family(FONT_MONO)
                        .child(t.updated_at.clone()),
                )
                .child(div().flex_1());'''
assert old in s, "inbox row not found"
s = s.replace(old, new)

# --- 4. library rows: borderless
old = '''            let mut row = div()
                .id(sid(format!("brief-{tid}")))
                .flex()
                .flex_col()
                .gap_0p5()
                .px_3()
                .py_2()
                .rounded_sm()
                .bg(c(PANEL))
                .border_1()
                .border_color(c(BORDER));
            row = row.hover(|s| s.bg(c(PANEL2)).border_color(c(ACCENT)));'''
new = '''            let mut row = div()
                .id(sid(format!("brief-{tid}")))
                .flex()
                .flex_col()
                .gap_0p5()
                .px_3()
                .py_2()
                .rounded_sm()
                .bg(c(PANEL));
            row = row.hover(|s| s.bg(c(PANEL2)));'''
assert old in s, "library row not found"
s = s.replace(old, new)

# --- 5. audit rows: borderless
old = '''            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .bg(c(PANEL))
                    .border_1()
                    .border_color(c(BORDER))'''
new = '''            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .bg(c(PANEL))'''
assert old in s, "audit row not found"
s = s.replace(old, new)

# --- 6. toast strip: tighten double padding
old = '''        let mut strip = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_1p5()
            .bg(c(PANEL))
            .border_b_1()
            .border_color(c(BORDER));'''
new = '''        let mut strip = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_1()
            .bg(c(PANEL))
            .border_b_1()
            .border_color(c(BORDER));'''
assert old in s, "toast strip not found"
s = s.replace(old, new)

# --- 7. refresh button glyph
old = '''            .child(
                self.button("\u5237\u65b0\u961f\u5217", BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {
                    ws.reload_queue(cx);
                }),
            )'''
new = '''            .child(
                self.button("\\u{E72C} \u5237\u65b0\u961f\u5217", BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {
                    ws.reload_queue(cx);
                }),
            )'''
assert old in s, "refresh button not found"
s = s.replace(old, new)

# --- 8. quadrant chips: borderless
old = '''                    cell = cell.child(
                        div()
                            .id(sid(format!("q-{tid}")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(c(PANEL2))
                            .hover(|s| s.border_color(c(ACCENT)))
                            .border_1()
                            .border_color(c(BORDER))'''
new = '''                    cell = cell.child(
                        div()
                            .id(sid(format!("q-{tid}")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(c(PANEL2))
                            .hover(|s| s.bg(c(BTN_HOVER)))'''
assert old in s, "quadrant chip not found"
s = s.replace(old, new)

open(p, 'w', encoding='utf-8').write(s)

# ============ detail.rs ============
p = 'crates/wb-app/src/detail.rs'
s = open(p, encoding='utf-8').read()

# --- 9. modal centered over main pane (DPI-aware margin)
old = '''        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .w(px(880.0))'''
new = '''        let avail = (window.viewport_size().width / window.scale_factor()) - px(220.0 + 16.0 * 2.0);
        let modal_ml = ((avail - px(880.0)) / 2.0).max(px(0.0));
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .ml(modal_ml)
                    .w(px(880.0))'''
assert old in s, "modal wrapper not found"
s = s.replace(old, new)

# --- 10. back button glyph
old = 'let back = self.button("\u2190 \u8fd4\u56de\u5217\u8868", BadgeTone::Neutral, false, window, cx, |ws, _ev, _w, cx| {'
new = 'let back = self.button("\\u{E72B} \u8fd4\u56de\u5217\u8868", BadgeTone::Neutral, false, window, cx, |ws, _ev, _w, cx| {'
assert old in s, "back button not found"
s = s.replace(old, new)

# export button glyph
old = '''                        self.button(
                            "\u5bfc\u51fa JSON",'''
new = '''                        self.button(
                            "\\u{E898} \u5bfc\u51fa JSON",'''
assert old in s, "export button not found"
s = s.replace(old, new)

# evidence collapse glyph
old = '''                .child(self.button("\u6536\u8d77", BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {'''
new = '''                .child(self.button("\\u{E738} \u6536\u8d77", BadgeTone::Neutral, false, _w, cx, |ws, _e, _w, cx| {'''
assert old in s, "collapse button not found"
s = s.replace(old, new)

# --- 11. line height on paragraph text
old = '''        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .py_1()
            .text_size(px(13.5))
            .text_color(c(FG));'''
new = '''        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .py_1()
            .text_size(px(14.0))
            .line_height(px(22.0))
            .text_color(c(FG));'''
assert old in s, "inference body not found"
s = s.replace(old, new)

old = '''        let mut body = div().flex().flex_col().gap_1().text_size(px(13.0));
        for fact in facts {'''
new = '''        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(13.0))
            .line_height(px(20.0));
        for fact in facts {'''
assert old in s, "fact body not found"
s = s.replace(old, new)

open(p, 'w', encoding='utf-8').write(s)

# ============ normalize fractional text sizes across app ============
for p in ['crates/wb-app/src/workspace.rs', 'crates/wb-app/src/detail.rs']:
    s = open(p, encoding='utf-8').read()
    s = s.replace('text_size(px(10.5))', 'text_size(px(11.0))')
    s = s.replace('text_size(px(11.5))', 'text_size(px(12.0))')
    s = s.replace('text_size(px(12.5))', 'text_size(px(13.0))')
    open(p, 'w', encoding='utf-8').write(s)

print("all design patches applied")
