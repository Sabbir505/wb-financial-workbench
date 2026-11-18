//! Review analytics view (F13): quality metrics + model-error taxonomy from
//! the local audit trail. Everything is computed from the JSONL the workbench
//! itself writes — no server, no model in the loop.

use crate::i18n::{self, Lang};
use crate::theme::*;
use crate::workspace::Workspace;
use gpui::{div, prelude::*, px, Context, Hsla};
use wb_core::analytics::{self, AnalyticsSummary, ErrorCategory};

impl Workspace {
    pub fn render_analytics(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let records = self.audit.list();
        let summary = analytics::analyze(&records);

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
                        .child(match self.lang {
                            Lang::Zh => format!("{}（{} 条记录）", ts.analytics_title, records.len()),
                            Lang::En => format!("{} ({} records)", ts.analytics_title, records.len()),
                        }),
                )
                .child(
                    div()
                        .text_size(px(T12))
                        .text_color(c(FG2))
                        .child(ts.analytics_hint),
                ),
        );

        if records.is_empty() {
            pane = pane.child(
                div()
                    .p_8()
                    .text_size(px(T13))
                    .text_color(c(FG2))
                    .child(ts.an_empty),
            );
            return pane;
        }

        let mut scroll = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px_5()
            .pb_5()
            .gap_4()
            .id("analytics-scroll")
            .overflow_y_scroll();

        // Stat cards: dot + big number + caption. No filled blocks.
        scroll = scroll.child(self.render_stat_row(&summary, cx));
        scroll = scroll.child(self.render_taxonomy(&summary, cx));
        scroll = scroll.child(self.render_company_table(&summary, cx));
        if !summary.top_tokens.is_empty() {
            scroll = scroll.child(self.render_tokens(&summary));
        }
        pane.child(scroll)
    }

    fn render_stat_row(&self, s: &AnalyticsSummary, _cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let pct = |v: f64| format!("{:.0}%", v * 100.0);
        let cards: [(String, String, Hsla); 5] = [
            (ts.an_total.to_string(), s.total_reviews.to_string(), c(FG)),
            (ts.an_first_pass.to_string(), pct(s.first_pass_rate), c(GREEN)),
            (ts.an_revision_rate.to_string(), pct(s.revision_rate), c(ACCENT)),
            (ts.an_flags.to_string(), s.guardrail_flagged.to_string(), c(AMBER)),
            (ts.an_confirmed.to_string(), s.guardrail_confirmed.to_string(), c(GREEN)),
        ];
        let mut row = div().flex().flex_row().gap_3().flex_wrap();
        for (label, value, color) in cards {
            row = row.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .px_3()
                    .py_2p5()
                    .min_w(px(128.0))
                    .rounded_lg()
                    .bg(c(RAISED))
                    .border_1()
                    .border_color(c(HAIRLINE))
                    .child(
                        div()
                            .text_size(px(T11))
                            .text_color(c(FG3))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(px(T18))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .font_family(FONT_MONO)
                            .text_color(color)
                            .child(value),
                    ),
            );
        }
        row
    }

    fn cat_label(&self, cat: ErrorCategory) -> String {
        let ts = i18n::strings(self.lang);
        match cat {
            ErrorCategory::StaleData => ts.cat_stale.to_string(),
            ErrorCategory::HallucinatedNumber => ts.cat_hallucinated.to_string(),
            ErrorCategory::Arithmetic => ts.cat_arithmetic.to_string(),
            ErrorCategory::Wording => ts.cat_wording.to_string(),
            ErrorCategory::Other => ts.cat_other.to_string(),
        }
    }

    fn render_taxonomy(&self, s: &AnalyticsSummary, _cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let max = s.taxonomy.iter().map(|(_, n)| *n).max().unwrap_or(0).max(1);
        let hues = [
            c(AMBER),   // stale data
            c(RED),     // hallucinated number
            c(ACCENT),  // arithmetic
            c(FG2),     // wording
            c(FG3),     // other
        ];
        let mut wrap = div().flex().flex_col().gap_2().child(
            div()
                .text_size(px(T13))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(c(FG))
                .child(ts.an_taxonomy),
        );
        for (i, (cat, n)) in s.taxonomy.iter().enumerate() {
            let w = px(40.0 + 420.0 * (*n as f32 / max as f32));
            wrap = wrap.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(120.0))
                            .text_size(px(T12))
                            .text_color(c(FG2))
                            .truncate()
                            .child(self.cat_label(*cat)),
                    )
                    .child(
                        div()
                            .w(w)
                            .h(px(14.0))
                            .rounded_sm()
                            .bg(hues[i])
                            .opacity(if *n == 0 { 0.0 } else { 0.85 }),
                    )
                    .child(
                        div()
                            .text_size(px(T12))
                            .font_family(FONT_MONO)
                            .text_color(c(FG))
                            .child(n.to_string()),
                    ),
            );
        }
        wrap
    }

    fn render_company_table(&self, s: &AnalyticsSummary, _cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut wrap = div().flex().flex_col().gap_1().child(
            div()
                .text_size(px(T13))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(c(FG))
                .child(ts.an_per_company),
        );
        wrap = wrap.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .h(px(24.0))
                .items_center()
                .border_b_1()
                .border_color(c(HAIRLINE))
                .text_size(px(T11))
                .text_color(c(FG3))
                .child(div().w(px(140.0)).child(ts.col_company))
                .child(div().w(px(72.0)).child(ts.an_total))
                .child(div().w(px(72.0)).child(ts.approve))
                .child(div().w(px(96.0)).child(ts.approve_as_revised))
                .child(div().w(px(72.0)).child(ts.reject)),
        );
        for cs in &s.per_company {
            wrap = wrap.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .h(px(28.0))
                    .items_center()
                    .text_size(px(T12))
                    .child(
                        div()
                            .w(px(140.0))
                            .text_color(c(FG))
                            .truncate()
                            .child(cs.company.clone()),
                    )
                    .child(
                        div()
                            .w(px(72.0))
                            .font_family(FONT_MONO)
                            .text_color(c(FG))
                            .child(cs.reviews.to_string()),
                    )
                    .child(
                        div()
                            .w(px(72.0))
                            .font_family(FONT_MONO)
                            .text_color(c(GREEN))
                            .child(cs.approvals.to_string()),
                    )
                    .child(
                        div()
                            .w(px(96.0))
                            .font_family(FONT_MONO)
                            .text_color(c(ACCENT))
                            .child(cs.revisions.to_string()),
                    )
                    .child(
                        div()
                            .w(px(72.0))
                            .font_family(FONT_MONO)
                            .text_color(c(RED))
                            .child(cs.rejects.to_string()),
                    ),
            );
        }
        wrap
    }

    fn render_tokens(&self, s: &AnalyticsSummary) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut wrap = div().flex().flex_col().gap_1().child(
            div()
                .text_size(px(T13))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(c(FG))
                .child(ts.an_tokens),
        );
        for (token, n) in s.top_tokens.iter().take(8) {
            wrap = wrap.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(c(RED_TINT))
                            .font_family(FONT_MONO)
                            .text_size(px(T12))
                            .text_color(c(RED))
                            .child(token.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(T12))
                            .text_color(c(FG2))
                            .child(format!("×{n}")),
                    ),
            );
        }
        wrap
    }
}
