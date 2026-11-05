//! Minimal text input for GPUI (keystroke capture + clipboard paste).
//! Note: IME composition (Chinese typing) is out of scope for this build —
//! paste (Ctrl+V) works, and review flows are pre-filled from the original draft.

use crate::theme::{c, icon, ACCENT, BORDER, DIM, FG, FG3, FONT_MONO, PANEL2};
use gpui::{
    div, prelude::*, px, Context, FocusHandle, InteractiveElement, KeyDownEvent, Render,
    SharedString, Styled, Window,
};

/// Change callback: the (new) content plus the GPUI window/app contexts.
pub type OnChange = Box<dyn Fn(&str, &mut Window, &mut gpui::App)>;

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

    fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.content.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.emit_change(window, cx);
    }

    fn backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        cx.notify();
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let before = self.content[..self.cursor].to_string();
        let after = self.content[self.cursor..].to_string();
        let empty = self.content.is_empty();
        let mono = self.font_mono;
        let mut base = div()
            .id("wb-text-input")
            .track_focus(&self.focus)
            .flex()
            .flex_row()
            .flex_wrap()
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
            .on_click(cx.listener(|this, _ev, window, _cx| {
                this.focus.focus(window);
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {                let key = ev.keystroke.key.clone();
                let mods = ev.keystroke.modifiers;
                if (mods.control || mods.platform) && key == "v" {
                    if let Some(item) = cx.read_from_clipboard() {
                        let text = item.text().unwrap_or_default();
                        this.insert(&text, window, cx);
                    }
                    return;
                }
                if mods.control || mods.platform || mods.alt {
                    return;
                }
                match key.as_ref() {
                    "backspace" => this.backspace(window, cx),
                    "delete" => this.delete(window, cx),
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
                            this.insert("\n", window, cx);
                        }
                    }
                    "space" => this.insert(" ", window, cx),
                    "tab" => {}
                    "up" | "down" | "escape" => {}
                    single
                        if single.chars().count() == 1
                            && !single.chars().next().unwrap().is_control() =>
                    {
                        this.insert(single, window, cx);
                    }
                    _ => {}
                }
            }));

        if let Some(glyph) = self.icon {
            base = base.child(icon(glyph, 13.0, c(FG3)).mr(px(6.0)));
        }

        if empty {
            base.child(div().text_color(c(DIM)).child(self.placeholder.clone()))
        } else {
            base.child(div().child(before))
                .child(div().w(px(2.0)).h(px(16.0)).bg(c(FG)))
                .child(div().child(after))
        }
    }
}
