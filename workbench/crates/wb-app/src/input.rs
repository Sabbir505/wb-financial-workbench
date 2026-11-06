//! Text input for GPUI：键盘编辑 + 粘贴 + **IME 组合输入（中文/日文等）**。
//!
//! 输入通道分工（GPUI Windows 平台层行为）：
//! - 普通字符（WM_CHAR，含 IME 提交文本）→ `InputHandler.replace_text_in_range`；
//! - IME 组合串（预编辑）→ `replace_and_mark_text_in_range`，以**下划线**渲染，提交后清除；
//! - 控制键（退格/删除/方向/回车，控制字符被平台过滤）→ 仍走 `on_key_down`；
//! 因此按键处理里**不再**直接插入普通字符，避免与 WM_CHAR 通道双重输入。

use crate::theme::{c, icon, ACCENT, ACCENT_DIM, BORDER, DIM, FG, FG3, FONT_MONO, PANEL2, SURFACE};
use gpui::{
    div, fill, point, prelude::*, px, size, Bounds, Context, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, GlobalElementId, InteractiveElement, KeyDownEvent,
    LayoutId, MouseButton, PaintQuad, Pixels, Render, ShapedLine, SharedString, Style, Styled,
    TextRun, UTF16Selection, UnderlineStyle, Window,
};
use std::ops::Range;

/// Change callback: the (new) content plus the GPUI window/app contexts.
pub type OnChange = Box<dyn Fn(&str, &mut Window, &mut gpui::App)>;

// ---------------------------------------------------------------------------
// 纯文本模型（自由函数，零 GPUI 依赖，可直接单测）
// ---------------------------------------------------------------------------

/// UTF-8 字节偏移 → UTF-16 单元偏移（Windows IME 口径）。
fn to_utf16_offset(s: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    let mut utf8 = 0;
    for ch in s.chars() {
        if utf8 >= offset {
            break;
        }
        utf8 += ch.len_utf8();
        utf16 += ch.len_utf16();
    }
    utf16
}

/// UTF-16 单元偏移 → UTF-8 字节偏移。
fn from_utf16_offset(s: &str, offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in s.chars() {
        if utf16 >= offset {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

/// 向前吸附到最近的字符边界（防止非边界范围在 replace_range 时 panic）。
fn snap_floor(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 编辑目标范围的统一解析：显式范围（UTF-16 换算）> IME 组合串 > 当前选择 > 光标处。
fn resolve_edit_range(
    content: &str,
    explicit_utf16: Option<&Range<usize>>,
    marked: Option<&Range<usize>>,
    selection: &Range<usize>,
    cursor: usize,
) -> Range<usize> {
    if let Some(r) = explicit_utf16 {
        return from_utf16_offset(content, r.start)..from_utf16_offset(content, r.end);
    }
    if let Some(m) = marked {
        return m.clone();
    }
    if selection.start < selection.end {
        return selection.clone();
    }
    cursor..cursor
}

/// 替换范围文本并返回新光标位置；范围先钳制到字符边界（宽容 IME 边界差异）。
fn splice_str(content: &mut String, range: Range<usize>, new_text: &str) -> usize {
    let start = snap_floor(content, range.start.min(content.len()));
    let end = snap_floor(content, range.end.min(content.len())).max(start);
    content.replace_range(start..end, new_text);
    start + new_text.len()
}

pub struct TextInput {
    pub content: String,
    /// Byte offset into content, always on a char boundary.
    pub cursor: usize,
    pub multiline: bool,
    pub placeholder: SharedString,
    pub font_mono: bool,
    /// Optional leading glyph (e.g. a search icon) that marks the field as
    /// interactive.
    pub icon: Option<&'static str>,
    pub focus: FocusHandle,
    on_change: Option<OnChange>,
    /// IME 组合串（预编辑）范围（UTF-8 字节偏移）；提交后清除。
    marked_range: Option<Range<usize>>,
    /// 各行 ShapedLine 与其在 content 中的起始 UTF-8 偏移（供 IME 弹窗定位/点击定位）。
    last_layout: Vec<(ShapedLine, usize)>,
    last_bounds: Option<Bounds<Pixels>>,
    /// 当前选择范围（字节偏移；空范围 = 无选择）。供剪切/复制/全选/选择删除。
    selected_range: Range<usize>,
    /// 右键菜单是否展开（粘贴/剪切/复制/全选，锚定在输入框下方）。
    ctx_menu_open: bool,
}

impl TextInput {
    pub fn new(
        multiline: bool,
        placeholder: impl Into<SharedString>,
        font_mono: bool,
        cx: &mut Context<Self>,
    ) -> TextInput {
        TextInput {
            content: String::new(),
            cursor: 0,
            multiline,
            placeholder: placeholder.into(),
            font_mono,
            icon: None,
            focus: cx.focus_handle(),
            on_change: None,
            marked_range: None,
            last_layout: Vec::new(),
            last_bounds: None,
            selected_range: 0..0,
            ctx_menu_open: false,
        }
    }

    pub fn with_icon(mut self, glyph: &'static str) -> Self {
        self.icon = Some(glyph);
        self
    }

    pub fn with_content(mut self, content: &str) -> Self {
        self.content = content.to_string();
        self.cursor = self.content.len();
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_on_change(
        &mut self,
        cb: impl Fn(&str, &mut Window, &mut gpui::App) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.on_change = Some(Box::new(cb));
        cx.notify();
    }

    fn emit_change(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
        if let Some(cb) = &self.on_change {
            cb(&self.content.clone(), window, cx);
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selected_range.start < self.selected_range.end
    }

    fn backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let r = self.selected_range.clone();
            self.splice(r, "");
            self.emit_change(window, cx);
            return;
        }
        if let Some(prev) = self.content[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
        {
            self.content.replace_range(prev..self.cursor, "");
            self.cursor = prev;
            self.emit_change(window, cx);
        }
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let r = self.selected_range.clone();
            self.splice(r, "");
            self.emit_change(window, cx);
            return;
        }
        if self.cursor < self.content.len() {
            let next = self.content[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.content.len());
            self.content.replace_range(self.cursor..next, "");
            self.emit_change(window, cx);
        }
    }

    fn move_cursor(&mut self, delta: isize, _window: &mut Window, cx: &mut Context<Self>) {
        let target = if delta < 0 {
            self.content[..self.cursor]
                .char_indices()
                .nth_back((-delta - 1) as usize)
                .map(|(i, _)| i)
        } else {
            self.content[self.cursor..]
                .char_indices()
                .nth(delta as usize)
                .map(|(i, _)| self.cursor + i)
        };
        self.cursor = target.unwrap_or(if delta < 0 { 0 } else { self.content.len() });
        self.selected_range = self.cursor..self.cursor; // 方向键收起选择（Windows 行为）
        cx.notify();
    }

    fn insert_newline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let r = self.selected_range.clone();
            self.splice(r, "\n");
        } else {
            self.content.insert(self.cursor, '\n');
            self.cursor += 1;
        }
        self.emit_change(window, cx);
    }

    /// 菜单动作：全选。
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.cursor = self.content.len();
        self.ctx_menu_open = false;
        cx.notify();
    }

    /// 菜单动作：粘贴（有选择则替换选择，否则光标处插入）。
    pub fn menu_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            let text = item.text().unwrap_or_default();
            self.replace_text_in_range(None, &text, window, cx);
        }
        self.ctx_menu_open = false;
        cx.notify();
    }

    /// 菜单动作：剪切/复制当前选择到剪贴板（剪切同时删除选择）。
    pub fn menu_cut_copy(&mut self, cut: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let txt = self.content[self.selected_range.clone()].to_string();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(txt));
            if cut {
                let r = self.selected_range.clone();
                self.splice(r, "");
                self.emit_change(window, cx);
            }
        }
        self.ctx_menu_open = false;
        cx.notify();
    }

    // ---- UTF-16 ↔ UTF-8 偏移换算（委托纯函数 to_utf16_offset/from_utf16_offset） ----

    fn offset_to_utf16(&self, offset: usize) -> usize {
        to_utf16_offset(&self.content, offset)
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        from_utf16_offset(&self.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    /// 用 new_text 替换 range（字节偏移），光标落到替换后文本末尾；任何内容变更
    /// 都会使旧选择失效（Windows 行为：编辑后选择区坍缩到光标）。
    fn splice(&mut self, range: Range<usize>, new_text: &str) {
        self.cursor = splice_str(&mut self.content, range, new_text);
        self.selected_range = self.cursor..self.cursor;
    }

    /// 编辑目标范围的统一解析（委托纯函数 resolve_edit_range，可单测）：
    /// 显式范围 > IME 组合串 > 当前选择 > 光标处。
    fn resolve_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        resolve_edit_range(
            &self.content,
            range_utf16.as_ref(),
            self.marked_range.as_ref(),
            &self.selected_range,
            self.cursor,
        )
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        let end = range.end.min(self.content.len());
        let start = range.start.min(end);
        actual_range.replace(self.range_to_utf16(&(start..end)));
        Some(self.content[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let c = self.offset_to_utf16(self.cursor);
        Some(UTF16Selection {
            range: c..c,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.resolve_range(range_utf16);
        self.splice(range, new_text);
        self.marked_range = None;
        self.emit_change(window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.resolve_range(range_utf16);
        let start = range.start.min(self.content.len());
        self.splice(range, new_text);
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(start..start + new_text.len())
        };
        self.emit_change(window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // IME 弹窗定位：精确到组合串所在行的起止 x；布局缺失时退化为整框
        let range = self.range_from_utf16(&range_utf16);
        let (line, line_start, line_idx) = self
            .last_layout
            .iter()
            .enumerate()
            .filter(|(_, (_, s))| *s <= range.start)
            .last()
            .map(|(i, (l, s))| (l, *s, i))?;
        let x0 = line.x_for_index(range.start.saturating_sub(line_start));
        let x1 = line.x_for_index(range.end.saturating_sub(line_start));
        let line_height = bounds.size.height / self.last_layout.len().max(1) as f32;
        Some(Bounds::from_corners(
            point(bounds.left() + x0, bounds.top() + line_height * line_idx as f32),
            point(
                bounds.left() + x1,
                bounds.top() + line_height * (line_idx + 1) as f32,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let local_y = point.y - bounds.top();
        let line_height = bounds.size.height / self.last_layout.len().max(1) as f32;
        let idx = (local_y / line_height).floor().max(0.0) as usize;
        let (line, line_start) = self.last_layout.get(idx)?;
        let x = point.x - bounds.left();
        let col = line.closest_index_for_x(x);
        Some(self.offset_to_utf16(line_start + col))
    }
}

// ---------------------------------------------------------------------------
// 自绘元素：文本行渲染 + IME 组合串下划线 + 光标 + IME 注册
// ---------------------------------------------------------------------------

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    lines: Vec<(ShapedLine, usize, gpui::Point<Pixels>)>,
    highlights: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let n_lines = self.input.read(cx).content.split('\n').count().max(1);
        let mut style = Style::default();
        style.size.width = gpui::relative(1.).into();
        style.size.height = (window.line_height() * n_lines as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let empty = input.content.is_empty();
        let display = if empty {
            input.placeholder.to_string()
        } else {
            input.content.clone()
        };
        let style = window.text_style();
        let base_color = if empty { c(DIM) } else { style.color };
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let mut lines = Vec::new();
        let mut highlights = Vec::new();
        let sel = &input.selected_range;
        let mut offset = 0usize;
        for (line_idx, seg) in display.split('\n').enumerate() {
            // IME 组合串下划线（仅处理与组合串相交的行，跨行组合罕见，截断到行内）
            let seg_start = offset;
            let seg_end = seg_start + seg.len();
            let runs = if let Some(marked) = input.marked_range.as_ref() {
                let ms = marked.start.clamp(seg_start, seg_end);
                let me = marked.end.clamp(seg_start, seg_end);
                if ms < me {
                    vec![
                        TextRun {
                            len: ms - seg_start,
                            font: font.clone(),
                            color: base_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        },
                        TextRun {
                            len: me - ms,
                            font: font.clone(),
                            color: base_color,
                            background_color: None,
                            underline: Some(UnderlineStyle {
                                color: Some(base_color),
                                thickness: px(1.0),
                                wavy: false,
                            }),
                            strikethrough: None,
                        },
                        TextRun {
                            len: seg_end - me,
                            font: font.clone(),
                            color: base_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        },
                    ]
                    .into_iter()
                    .filter(|r| r.len > 0)
                    .collect()
                } else {
                    vec![TextRun {
                        len: seg.len(),
                        font: font.clone(),
                        color: base_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }]
                }
            } else {
                vec![TextRun {
                    len: seg.len(),
                    font: font.clone(),
                    color: base_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }]
            };
            let shaped = window
                .text_system()
                .shape_line(SharedString::from(seg.to_string()), font_size, &runs, None);
            let origin = point(bounds.left(), bounds.top() + line_height * line_idx as f32);
            // 选择高亮（按行截断到行内范围）
            if !empty && sel.start < sel.end {
                let s0 = sel.start.clamp(seg_start, seg_end);
                let s1 = sel.end.clamp(seg_start, seg_end);
                if s0 < s1 {
                    let x0 = shaped.x_for_index(s0 - seg_start);
                    let x1 = shaped.x_for_index(s1 - seg_start);
                    highlights.push(fill(
                        Bounds::new(point(origin.x + x0, origin.y), size(x1 - x0, line_height)),
                        gpui::rgba(0x2f6bd833),
                    ));
                }
            }
            lines.push((shaped, seg_start, origin));
            offset = seg_end + 1;
        }

        // 光标（仅在聚焦且无选择时绘制；有选择时隐藏光标，与 Windows 行为一致）
        let cursor = if input.focus.is_focused(window) && !empty && sel.start == sel.end {
            let cursor = input.cursor;
            let (line, line_start, origin) = lines
                .iter()
                .enumerate()
                .filter(|(_, (_, s, _))| *s <= cursor)
                .last()
                .map(|(_, (l, s, o))| (l, *s, *o))
                .unwrap_or((&lines[0].0, lines[0].1, lines[0].2));
            let x = line.x_for_index(cursor.saturating_sub(line_start));
            Some(fill(
                Bounds::new(
                    point(origin.x + x, origin.y),
                    size(px(2.0), line_height),
                ),
                c(FG),
            ))
        } else {
            None
        };

        PrepaintState {
            lines,
            highlights,
            cursor,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let focus_handle = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for quad in prepaint.highlights.drain(..) {
            window.paint_quad(quad);
        }
        for (line, _, origin) in prepaint.lines.iter() {
            line.paint(*origin, window.line_height(), window, cx)
                .unwrap();
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _cx| {
            input.last_layout = prepaint
                .lines
                .iter()
                .map(|(l, s, _)| (l.clone(), *s))
                .collect();
            input.last_bounds = Some(bounds);
        });
    }
}

// ---------------------------------------------------------------------------
// Render：外框（背景/边框/聚焦态） + 图标 + TextElement
// ---------------------------------------------------------------------------

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = self.font_mono;
        let mut base = div()
            .id("wb-text-input")
            .track_focus(&self.focus)
            .flex()
            .flex_row()
            .min_w_full()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(c(PANEL2))
            .border_1()
            .border_color(c(BORDER))
            .when(self.focus.is_focused(window), |el| {
                el.border_color(c(ACCENT))
            })
            .text_size(px(13.0))
            .text_color(c(FG))
            .when(mono, |el| el.font_family(FONT_MONO))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _ev, window, cx| {
                // 右键：聚焦并展开操作菜单（粘贴/剪切/复制/全选）
                this.focus.focus(window);
                this.ctx_menu_open = true;
                cx.notify();
                cx.stop_propagation();
            }))
            .on_click(cx.listener(|this, ev: &gpui::ClickEvent, window, cx| {
                this.focus.focus(window);
                this.ctx_menu_open = false; // 左键点击关闭右键菜单
                // 点击定位光标并收起选择（按 ShapedLine 命中列换算；CJK 宽字符同样准确）
                if let Some(utf16_idx) =
                    this.character_index_for_point(ev.position(), window, cx)
                {
                    this.cursor = this.offset_from_utf16(utf16_idx);
                    this.selected_range = this.cursor..this.cursor;
                }
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let key = ev.keystroke.key.clone();
                let mods = ev.keystroke.modifiers;
                if mods.control || mods.platform {
                    match key.as_ref() {
                        // Windows 文本编辑快捷键：粘贴/全选/复制/剪切
                        "v" => {
                            if let Some(item) = cx.read_from_clipboard() {
                                let text = item.text().unwrap_or_default();
                                this.replace_text_in_range(None, &text, window, cx);
                            }
                        }
                        "a" => {
                            this.selected_range = 0..this.content.len();
                            this.cursor = this.content.len();
                            cx.notify();
                        }
                        "c" => {
                            if this.has_selection() {
                                let txt = this.content[this.selected_range.clone()].to_string();
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(txt));
                            }
                        }
                        "x" => {
                            if this.has_selection() {
                                let txt = this.content[this.selected_range.clone()].to_string();
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(txt));
                                let r = this.selected_range.clone();
                                this.splice(r, "");
                                this.emit_change(window, cx);
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                if mods.alt {
                    return;
                }
                // IME 组合期间：退格/删除交给 IME 管理组合串，应用侧不干预
                let composing = this.marked_range.is_some();
                match key.as_ref() {
                    "backspace" if !composing => this.backspace(window, cx),
                    "delete" if !composing => this.delete(window, cx),
                    "left" => this.move_cursor(-1, window, cx),
                    "right" => this.move_cursor(1, window, cx),
                    "home" => {
                        this.cursor = 0;
                        cx.notify();
                    }
                    "end" => {
                        this.cursor = this.content.len();
                        cx.notify();
                    }
                    "enter" => {
                        if this.multiline {
                            this.insert_newline(window, cx);
                        }
                    }
                    "escape" => {
                        this.ctx_menu_open = false;
                        cx.notify();
                    }
                    // 普通字符（含空格）经 WM_CHAR → replace_text_in_range 进入，
                    // 不在此处插入，避免双重输入
                    _ => {}
                }
            }));

        if let Some(glyph) = self.icon {
            base = base.child(icon(glyph, 13.0, c(FG3)).mr(px(6.0)));
        }
        base = base.child(TextElement {
            input: cx.entity(),
        });

        // 右键操作菜单（deferred 浮层，锚定输入框下缘；项随上下文启停）
        if self.ctx_menu_open && self.last_bounds.is_some() {
            let b = self.last_bounds.unwrap();
            let has_sel = self.has_selection();
            let has_content = !self.content.is_empty();
            base = base.child(gpui::deferred(
                div()
                    .absolute()
                    .left(b.left())
                    .top(b.bottom() + px(4.0))
                    .flex()
                    .flex_col()
                    .min_w(px(116.0))
                    .bg(c(SURFACE))
                    .border_1()
                    .border_color(c(BORDER))
                    .rounded_md()
                    .shadow_lg()
                    .child(self.menu_row("粘贴", true, MenuAct::Paste, cx))
                    .child(self.menu_row("剪切", has_sel, MenuAct::Cut, cx))
                    .child(self.menu_row("复制", has_sel, MenuAct::Copy, cx))
                    .child(self.menu_row("全选", has_content, MenuAct::SelectAll, cx)),
            ));
        }
        base
    }
}

#[derive(Clone, Copy)]
enum MenuAct {
    Paste,
    Cut,
    Copy,
    SelectAll,
}

impl TextInput {
    fn menu_row(
        &self,
        label: &'static str,
        enabled: bool,
        act: MenuAct,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut d = div().px_3().py_1().text_size(px(12.0)).child(label);
        if enabled {
            d = d
                .text_color(c(FG))
                .cursor_pointer()
                .hover(|s| s.bg(c(ACCENT_DIM)))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _ev, window, cx| {
                    match act {
                        MenuAct::Paste => this.menu_paste(window, cx),
                        MenuAct::Cut => this.menu_cut_copy(true, window, cx),
                        MenuAct::Copy => this.menu_cut_copy(false, window, cx),
                        MenuAct::SelectAll => this.select_all(cx),
                    }
                    cx.stop_propagation();
                }));
        } else {
            d = d.text_color(c(DIM));
        }
        d
    }
}

// ---------------------------------------------------------------------------
// IME/选择路径的回归测试（纯文本模型自由函数，零 GPUI 上下文依赖）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_roundtrip_cjk_and_surrogate() {
        // 字节布局：斯[0..3] 迪[3..6] 克[6..9] a[9..10] 🚀[10..14]
        // UTF-16 布局：斯=1 迪=1 克=1 a=1 🚀=2（代理对），共 6 单元
        let s = "斯迪克a🚀";
        assert_eq!(to_utf16_offset(s, 0), 0);
        assert_eq!(to_utf16_offset(s, 9), 3); // 三个汉字后 = 3 单元
        assert_eq!(to_utf16_offset(s, 10), 4); // 'a' 后 = 4 单元
        assert_eq!(to_utf16_offset(s, s.len()), 6); // 全文 = 6 单元
        // 往返一致
        assert_eq!(from_utf16_offset(s, to_utf16_offset(s, s.len())), s.len());
        assert_eq!(from_utf16_offset(s, 2), 6); // 2 单元 = "斯迪" 后的字节偏移
    }

    #[test]
    fn resolve_range_precedence() {
        let content = "斯迪克新材料";
        // 默认：光标处空范围
        assert_eq!(resolve_edit_range(content, None, None, &(3..3), 3), 3..3);
        // 有选择时：选择优先于光标
        assert_eq!(resolve_edit_range(content, None, None, &(0..3), 3), 0..3);
        // IME 组合串优先于选择
        assert_eq!(resolve_edit_range(content, None, Some(&(3..6)), &(0..3), 3), 3..6);
        // 显式范围最高优先（UTF-16 0..2 = 两个汉字 = UTF-8 字节 0..6）
        assert_eq!(
            resolve_edit_range(content, Some(&(0..2)), Some(&(3..6)), &(0..3), 3),
            0..6
        );
    }

    #[test]
    fn splice_splices_and_returns_cursor() {
        // 0..3 = "斯"（3 字节），替换为"斯迪" → "斯迪" + "迪克新材料"
        let mut s = "斯迪克新材料".to_string();
        let cursor = splice_str(&mut s, 0..3, "斯迪");
        assert_eq!(s, "斯迪迪克新材料");
        assert_eq!(cursor, 6); // "斯迪" 6 字节
    }

    #[test]
    fn splice_snaps_non_boundary_safely() {
        // "斯"是 3 字节：0..2 不在字符边界——钳制而非 panic
        let mut s = "斯迪克".to_string();
        let cursor = splice_str(&mut s, 0..2, "");
        assert_eq!(s, "斯迪克"); // 0..2 向前吸附成 0..0，内容不变
        assert_eq!(cursor, 0);
    }
}
