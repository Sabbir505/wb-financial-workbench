//! WB design system v3 — principles applied from research:
//! • Fluent 2: 4px spacing grid, semantic type ramp (Caption 11 / Body 13 / Subtitle 15 / Title 18)
//! • Light-mode hierarchy: white surfaces with subtle gray steps; clear surface steps
//! • Color restraint: one interactive accent; status is a *dot + tinted text*, never a filled block
//! • Hairline borders only for structure and inputs; lists are transparent rows with hover lift

use gpui::{div, px, Hsla, ParentElement, SharedString, Styled};

// ---------------------------------------------------------------- surfaces（浅色）
pub const BG: u32 = 0xffffff; // window base（纯白）
pub const SURFACE: u32 = 0xffffff; // topbar / sidebar / panels
pub const RAISED: u32 = 0xeff2f6; // inputs, cards, modal, quadrant cells
pub const RAISED_HOVER: u32 = 0xe2e8f0; // hover on raised / list rows
pub const HAIRLINE: u32 = 0xe3e8ef; // structural 1px dividers

// ---------------------------------------------------------------- text（浅色底上的深色文字）
pub const FG: u32 = 0x1a1d24; // primary
pub const FG2: u32 = 0x55606e; // secondary
pub const FG3: u32 = 0x8a94a3; // tertiary / meta

// ---------------------------------------------------------------- accent (interactive only)
pub const ACCENT: u32 = 0x154f8b;
pub const ACCENT_DIM: u32 = 0xe3edfb; // accent tint on light

// ---------------------------------------------------------------- status hues（白底可读版）
pub const GREEN: u32 = 0x1e7e34;
pub const AMBER: u32 = 0x8a6d00;
pub const RED: u32 = 0xb71c1c;

// ---------------------------------------------------------------- tints (sparing use)
pub const RED_TINT: u32 = 0xfbe9e9;
pub const GREEN_TINT: u32 = 0xe6f4ea;
pub const AMBER_TINT: u32 = 0xfdf6e3;

pub fn c(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

// ---------------------------------------------------------------- type ramp
pub const T11: f32 = 11.0; // caption: meta, timestamps, badges
pub const T12: f32 = 12.0; // secondary body: labels, chips
pub const T13: f32 = 13.0; // body: rows, buttons, nav
pub const T14: f32 = 14.0; // large body: paragraphs
pub const T15: f32 = 15.0; // view titles
pub const T18: f32 = 18.0; // document title
pub const FONT_UI: &str = "Segoe UI";
pub const FONT_MONO: &str = "Consolas";
/// Segoe Fluent Icons (Windows 11 system font) for small UI glyphs.
pub const FONT_ICON: &str = "Segoe Fluent Icons";

/// A small monochrome glyph from Segoe Fluent Icons.
pub fn icon(code: &str, size: f32, color: Hsla) -> gpui::Div {
    div()
        .font_family(FONT_ICON)
        .text_size(px(size))
        .text_color(color)
        .child(code.to_string())
}

/// A small keycap: raised surface, hairline, mono label. Keyboard shortcuts
/// render as keycaps, never as glowing pills or plain glowing text.
pub fn keycap(label: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .px_1p5()
        .h(px(20.0))
        .rounded_sm()
        .bg(c(RAISED))
        .border_1()
        .border_color(c(HAIRLINE))
        .text_size(px(T11))
        .font_family(FONT_MONO)
        .text_color(c(FG2))
        .child(label.to_string())
}

/// Priority → quiet mono tag hue (P1 red-ish, P2 amber, P3 blue, else gray).
pub fn priority_color(p: &str) -> Hsla {
    match p {
        "P1" => c(RED),
        "P2" => c(AMBER),
        "P3" => c(ACCENT),
        _ => c(FG3),
    }
}

/// A 6px status dot + tinted 12px label. The only way status is rendered —
/// no filled blocks, no borders.
#[derive(gpui::IntoElement)]
pub struct Badge {
    pub text: SharedString,
    pub tone: BadgeTone,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>, tone: BadgeTone) -> Badge {
        Badge {
            text: text.into(),
            tone,
        }
    }
}

impl gpui::RenderOnce for Badge {
    fn render(self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> impl gpui::IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .w(px(6.0))
                    .h(px(6.0))
                    .rounded_full()
                    .bg(self.tone.fg()),
            )
            .child(
                div()
                    .text_size(px(T12))
                    .text_color(self.tone.fg())
                    .child(self.text),
            )
    }
}

pub const AMBER_BG: u32 = AMBER_TINT;
pub const GREEN_BG: u32 = GREEN_TINT;
pub const RED_BG: u32 = RED_TINT;
pub const CYAN: u32 = ACCENT;

/// `ElementId` accepts SharedString but not String; keeps `.id()` chains terse.
pub fn sid(s: String) -> SharedString {
    s.into()
}

// ---------------------------------------------------------------- compat aliases
// detail.rs / input.rs still reference the v2 names during the redesign.
pub const DIM: u32 = FG2;
pub const BORDER: u32 = HAIRLINE;
pub const PANEL2: u32 = RAISED;
pub const PANEL: u32 = SURFACE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeTone {
    Neutral,
    Blue,
    Amber,
    Green,
    Red,
    Cyan,
}

impl BadgeTone {
    pub fn fg(self) -> Hsla {
        match self {
            BadgeTone::Neutral => c(FG2),
            BadgeTone::Blue => c(ACCENT),
            BadgeTone::Amber => c(AMBER),
            BadgeTone::Green => c(GREEN),
            BadgeTone::Red => c(RED),
            BadgeTone::Cyan => c(ACCENT),
        }
    }
    pub fn bg(self) -> Hsla {
        match self {
            BadgeTone::Neutral => c(RAISED),
            BadgeTone::Blue => c(ACCENT_DIM),
            BadgeTone::Amber => c(AMBER_TINT),
            BadgeTone::Green => c(GREEN_TINT),
            BadgeTone::Red => c(RED_TINT),
            BadgeTone::Cyan => c(ACCENT_DIM),
        }
    }
}

pub fn status_tone(status: &str) -> BadgeTone {
    match status {
        "WAITING_REVIEW" | "待审核" => BadgeTone::Amber,
        "SUCCEEDED" | "已通过" => BadgeTone::Green,
        "FAILED" | "已失败" => BadgeTone::Red,
        "CANCELLED" | "已驳回" => BadgeTone::Neutral,
        "RUNNING" | "CONTEXT_READY" | "TASK_CREATED" => BadgeTone::Blue,
        _ => BadgeTone::Neutral,
    }
}

/// Verdict (stored Chinese contract values) → badge tone.
pub fn verdict_tone(verdict: &str) -> BadgeTone {
    match verdict {
        "通过" => BadgeTone::Green,
        "驳回" => BadgeTone::Red,
        _ => BadgeTone::Blue,
    }
}
