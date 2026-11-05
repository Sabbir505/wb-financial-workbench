//! Brief detail view (F2/F3/F4/F11), evidence drill-down panel (F11) and the
//! review modal (F5/F6: 通过 / 修改后通过 with diff / 驳回).

use crate::i18n::{self, Lang};
use crate::theme::*;
use crate::workspace::{review_idempotency_key, Btn, Workspace};
use gpui::{div, prelude::*, px, ClickEvent, Context, Hsla, Styled, Window};
use wb_core::diff::{diff_lines, DiffOp};
use wb_core::vault::SectionKind;
use wb_core::guardrail::{Guardrail, NumSpan};
use wb_core::model::Verdict;

impl Workspace {
    // ---------------------------------------------------------------- header

    pub fn render_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let back = self.button(
            ts.back,
            Btn::Ghost,
            Some("\u{E72B}"),
            window,
            cx,
            |ws, _ev, _w, cx| {
                // Back from the full page to the preview drawer; the loaded
                // detail stays warm for the panel.
                ws.detail_expanded = false;
                cx.notify();
            },
        );

        let mut panel = div().flex().flex_col().flex_1().min_h_0();

        if self.detail_loading {
            panel = panel.child(div().p_6().text_color(c(DIM)).child(ts.detail_loading));
            return panel;
        }
        let Some(detail) = self.detail.clone() else {
            panel = panel.child(div().p_6().text_color(c(RED)).child(format!(
                "{}{}",
                ts.detail_failed,
                self.detail_error.clone().unwrap_or_default()
            )));
            return panel;
        };

        let title = format!(
            "{} {}{}",
            detail.object_name, detail.period, ts.brief_suffix
        );
        let version = detail.brief_version.unwrap_or(1);

        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pb_3()
                .border_b_1()
                .border_color(c(BORDER))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .flex_wrap()
                        .child(back)
                        .child(
                            div()
                                .text_size(px(T18))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(c(FG))
                                .child(title),
                        )
                        .child(Badge::new(
                            format!(
                                "{} · v{}",
                                i18n::status_label(self.lang, &detail.status),
                                version
                            ),
                            status_tone(&detail.status),
                        ))
                        .child(self.cutoff_badge(&detail.data_cutoff, &detail.source_version)),
                )
        );

        // Review actions (F5) — only for reviewable tasks.
        if detail.status == "WAITING_REVIEW" {
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .py_2()
                    .child(self.button(
                        ts.approve,
                        Btn::Success,
                        None,
                        window,
                        cx,
                        |ws, _ev, _w, cx| {
                            ws.open_review(Verdict::Approve, cx);
                        },
                    ))
                    .child(self.button(
                        ts.approve_as_revised,
                        Btn::Primary,
                        None,
                        window,
                        cx,
                        |ws, _ev, _w, cx| {
                            ws.open_review(Verdict::ApproveAsRevised, cx);
                        },
                    ))
                    .child(self.button(
                        ts.reject,
                        Btn::DangerGhost,
                        None,
                        window,
                        cx,
                        |ws, _ev, _w, cx| {
                            ws.open_review(Verdict::Reject, cx);
                        },
                    )),
            );
        } else {
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .py_2()
                    .text_size(px(12.0))
                    .text_color(c(DIM))
                    .child(ts.not_reviewable.replace("{}", &detail.status)),
            );
        }

        // The four sections.
        let sections = &detail.sections;
        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .pt_2()
                .child(self.section_fact(&sections.fact.0))
                .child(self.section_calculation(sections))
                .child(self.section_inference(&sections.inference, &self.guardrail, cx))
                .child(self.section_check(&sections.check)),
        );

        // F11 evidence drill-down: inline section, toggled by the button.
        panel = panel.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .pt_3()
                .child(self.button(
                    if self.show_evidence {
                        ts.evidence_hide
                    } else {
                        ts.evidence_toggle
                    },
                    Btn::Ghost,
                    if self.show_evidence { Some("\u{E738}") } else { None },
                    window,
                    cx,
                    |ws, _e, _w, cx| {
                        ws.show_evidence = !ws.show_evidence;
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(DIM))
                        .child(ts.evidence_sub),
                ),
        );
        if self.show_evidence {
            panel = panel.child(self.render_evidence_panel(window, cx));
        }

        // System & audit metadata — collapsed by default so tracing IDs and
        // contract fields stay reachable without crowding the review.
        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_3()
                .child(self.button(
                    ts.meta_toggle,
                    Btn::Ghost,
                    Some(if self.show_meta { "\u{E70E}" } else { "\u{E70D}" }),
                    window,
                    cx,
                    |ws, _e, _w, cx| {
                        ws.show_meta = !ws.show_meta;
                        cx.notify();
                    },
                ))
                .when(self.show_meta, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .gap_2()
                                    .text_size(px(12.0))
                                    .text_color(c(DIM))
                                    .child(format!("{} {}", ts.meta_task, detail.task_id))
                                    .child(format!("{} {}", ts.meta_trace, detail.trace_id))
                                    .child(format!("{} {}", ts.meta_object, detail.object_id))
                                    .child(format!(
                                        "{} {}",
                                        ts.meta_workflow, detail.workflow_version
                                    )),
                            )
                            .child(self.frontmatter_grid(&detail))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(c(DIM))
                                    .child(ts.review_via_mcp),
                            ),
                    )
                }),
        );

        // Process timeline — collapsed; states in plain language, not enums.
        let mut hist_rows = div().flex().flex_col().gap_1();
        for h in &detail.history {
            let from_to = match &h.from {
                Some(f) => format!(
                    "{} → {}",
                    i18n::status_label(self.lang, f),
                    i18n::status_label(self.lang, &h.to)
                ),
                None => format!("→ {}", i18n::status_label(self.lang, &h.to)),
            };
            hist_rows = hist_rows.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_size(px(11.0))
                    .text_color(c(DIM))
                    .font_family(FONT_MONO)
                    .child(div().min_w(px(150.0)).child(h.at.clone()))
                    .child(div().min_w(px(240.0)).child(from_to))
                    .child(div().child(h.note.clone())),
            );
        }
        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_1()
                .child(self.button(
                    ts.history_title,
                    Btn::Ghost,
                    Some(if self.show_history { "\u{E70E}" } else { "\u{E70D}" }),
                    window,
                    cx,
                    |ws, _e, _w, cx| {
                        ws.show_history = !ws.show_history;
                        cx.notify();
                    },
                ))
                .when(self.show_history, |el| el.child(hist_rows)),
        );

        panel
    }

    /// Data cutoff as quiet meta text — freshness is not a status worth a
    /// colored block; stale cutoffs get amber text, never a fill.
    /// Right-side preview drawer (master-detail): the core review material —
    /// identity, verdict actions, FACT / CALCULATION / INFERENCE / CHECK —
    /// without leaving the inbox. Evidence drill-down and the state history
    /// stay in the full view.
    pub fn render_detail_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let close = self.button("", Btn::Ghost, Some("\u{E8BB}"), window, cx, |ws, _e, _w, cx| {
            ws.selected_task = None;
            ws.detail = None;
            ws.context = None;
            ws.evidence = None;
            ws.methodology = None;
            ws.guardrail = None;
            ws.show_evidence = false;
            ws.hovered_violation = None;
            ws.detail_expanded = false;
            cx.notify();
        });

        // Grows to fill everything right of the list (grow-last arrangement).
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(420.0))
            .h_full()
            .bg(c(SURFACE))
            .border_l_1()
            .border_color(c(HAIRLINE));

        if self.detail_loading {
            return panel.child(
                div()
                    .p_4()
                    .text_size(px(T13))
                    .text_color(c(DIM))
                    .child(ts.detail_loading),
            );
        }
        let Some(detail) = self.detail.clone() else {
            return panel.child(
                div()
                    .p_4()
                    .text_size(px(T13))
                    .text_color(c(RED))
                    .child(format!(
                        "{}{}",
                        ts.detail_failed,
                        self.detail_error.clone().unwrap_or_default()
                    )),
            );
        };

        let title = format!("{} {}{}", detail.object_name, detail.period, ts.brief_suffix);
        let version = detail.brief_version.unwrap_or(1);

        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .id("panel-scroll")
            .overflow_y_scroll()
            .overflow_x_hidden()
            .p_4()
            .pr_5()
            .gap_2();

        // Header: close on its own row (gpui grows only the last flex child),
        // then identity, status + cutoff, and the task id.
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .child(div().flex_1())
                .child(close),
        );
        body = body.child(
            div()
                .text_size(px(T15))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(c(FG))
                .truncate()
                .child(title),
        );
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(Badge::new(
                    format!(
                        "{} · v{}",
                        i18n::status_label(self.lang, &detail.status),
                        version
                    ),
                    status_tone(&detail.status),
                ))
                .child(self.cutoff_badge(&detail.data_cutoff, &detail.source_version)),
        );
        // Verdict actions (F5) — same handlers as the full view.
        if detail.status == "WAITING_REVIEW" {
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .py_1()
                    .child(self.button(
                        ts.approve,
                        Btn::Success,
                        None,
                        window,
                        cx,
                        |ws, _ev, _w, cx| {
                            ws.open_review(Verdict::Approve, cx);
                        },
                    ))
                    .child(self.button(
                        ts.approve_as_revised,
                        Btn::Primary,
                        None,
                        window,
                        cx,
                        |ws, _ev, _w, cx| {
                            ws.open_review(Verdict::ApproveAsRevised, cx);
                        },
                    ))
                    .child(self.button(
                        ts.reject,
                        Btn::DangerGhost,
                        None,
                        window,
                        cx,
                        |ws, _ev, _w, cx| {
                            ws.open_review(Verdict::Reject, cx);
                        },
                    )),
            );
        } else {
            body = body.child(
                div()
                    .py_1()
                    .text_size(px(T12))
                    .text_color(c(DIM))
                    .child(ts.not_reviewable.replace("{}", &detail.status)),
            );
        }

        // The four contract sections, built by the same renderers as the
        // full view so guardrail spans and hover hints behave identically.
        let sections = &detail.sections;
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .pt_1()
                .child(self.section_fact(&sections.fact.0))
                .child(self.section_calculation(sections))
                .child(self.section_inference(&sections.inference, &self.guardrail, cx))
                .child(self.section_check(&sections.check)),
        );

        // Evidence drill-down and state history live in the full page.
        body = body.child(
            div().flex().flex_row().pt_1().child(self.button(
                ts.expand_full,
                Btn::Ghost,
                Some("\u{E8A7}"),
                window,
                cx,
                |ws, _e, _w, cx| {
                    ws.detail_expanded = true;
                    cx.notify();
                },
            )),
        );

        panel.child(body)
    }

    pub fn cutoff_badge(&self, data_cutoff: &str, _source_version: &str) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let stale = chrono::NaiveDateTime::parse_from_str(data_cutoff, "%Y-%m-%dT%H:%M:%S")
            .ok()
            .map(|t| (chrono::Local::now().naive_local() - t).num_hours() > 72)
            .unwrap_or(false);
        let prefix = match self.lang {
            Lang::Zh => "数据截止 ",
            Lang::En => "data cutoff ",
        };
        let label = format!(
            "{prefix}{data_cutoff}{}",
            if stale { ts.cutoff_stale } else { "" }
        );
        div()
            .text_size(px(11.0))
            .font_family(FONT_MONO)
            .text_color(if stale { c(AMBER) } else { c(FG3) })
            .child(label)
    }

    /// Frontmatter (I2 contract) as quiet key-value rows, not chips. The IDs
    /// (task/trace/object/workflow) already sit in the meta line above; this
    /// grid carries the timestamps and versions. Two fixed columns — gpui's
    /// flex_wrap stretches free-width children across the pane.
    fn frontmatter_grid(&self, detail: &wb_core::model::TaskDetail) -> gpui::Div {
        let brief_version = detail
            .brief_version
            .map(|v| v.to_string())
            .unwrap_or_default();
        let rows: Vec<(String, String)> = vec![
            ("data_cutoff".into(), detail.data_cutoff.clone()),
            ("source_version".into(), detail.source_version.clone()),
            ("created_at".into(), detail.created_at.clone()),
            ("updated_at".into(), detail.updated_at.clone()),
            ("brief_version".into(), brief_version),
        ];
        let kv_cell = |k: &str, v: &str| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .flex_1()
                .min_w_0()
                .text_size(px(11.0))
                .child(div().min_w(px(92.0)).text_color(c(FG3)).child(k.to_string()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(FONT_MONO)
                        .text_color(c(FG2))
                        .truncate()
                        .child(v.to_string()),
                )
        };
        let mut grid = div()
            .flex()
            .flex_col()
            .gap_1()
            .py_2()
            .border_b_1()
            .border_color(c(BORDER));
        for pair in rows.chunks(2) {
            let mut row = div().flex().flex_row().gap_6();
            for (k, v) in pair {
                row = row.child(kv_cell(k, v));
            }
            if pair.len() == 1 {
                row = row.child(div().flex_1());
            }
            grid = grid.child(row);
        }
        grid
    }

    // -------------------------------------------------------------- sections

    /// Section header: quiet secondary label with a small marker dot. Hue only
    /// where it carries the trust boundary (INFERENCE = model-generated amber);
    /// tool output and checks stay neutral.
    fn section_frame(&self, label: &str, dot: Hsla) -> gpui::Div {
        div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(dot))
                .child(
                    div()
                        .text_size(px(T12))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(c(FG2))
                        .child(label.to_string()),
                ),
        )
    }

    fn section_fact(&self, facts: &[String]) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut sec = self.section_frame(ts.sec_fact, c(FG3));
        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(13.0))
            .line_height(px(20.0));
        for fact in facts {
            let is_link = fact.contains("http");
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .when(is_link, |el| el.font_family(FONT_MONO).text_size(px(12.0)))
                    .child(div().text_color(c(CYAN)).child("·"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(if is_link { c(ACCENT) } else { c(FG) })
                            .child(fact.clone()),
                    ),
            );
        }
        sec = sec.child(body);
        sec
    }

    fn section_calculation(&self, sections: &wb_core::model::BriefSections) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut sec = self.section_frame(ts.sec_calc, c(FG3));
        let rows: Vec<(String, String)> = sections
            .calculation
            .0
            .iter()
            .map(|(k, v)| (k.clone(), fmt_value(v)))
            .collect();
        sec = sec.child(self.metric_grid(rows));
        sec
    }

    /// Metric grid: label over value in three columns — a ledger row a
    /// financial reviewer can scan, not a pile of pills.
    fn metric_grid(&self, rows: Vec<(String, String)>) -> gpui::Div {
        let mut grid = div().flex().flex_col().gap_2().pt_1();
        for chunk in rows.chunks(3) {
            let mut row = div().flex().flex_row().gap_6();
            for (k, v) in chunk {
                row = row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(px(T11))
                                .text_color(c(FG3))
                                .truncate()
                                .child(k.clone()),
                        )
                        .child(
                            div()
                                .font_family(FONT_MONO)
                                .text_size(px(T13))
                                .text_color(c(ACCENT))
                                .child(v.clone()),
                        ),
                );
            }
            for _ in chunk.len()..3 {
                row = row.child(div().flex_1());
            }
            grid = grid.child(row);
        }
        grid
    }

    /// INFERENCE with the F4 guardrail: every number is checked against evidence;
    /// misses are red-underlined, hover shows why (via the strip below).
    fn section_inference(
        &self,
        inference: &str,
        guardrail: &Option<Guardrail>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut sec = self.section_frame(ts.sec_inference, c(AMBER));

        if let Some(g) = guardrail {
            let (dot, fg) = if g.ok() {
                (c(GREEN), c(GREEN))
            } else {
                (c(RED), c(RED))
            };
            // Guardrail verdict as the app's status language: dot + tinted
            // text, never a filled banner. On failure the offending tokens in
            // the summary are hover-linked to the same hint as the flagged
            // numbers in the text below.
            let mut line = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_1p5()
                .py_0p5()
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(dot));
            if g.ok() {
                line = line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(T12))
                        .text_color(fg)
                        .child(ts.guard_ok.to_string()),
                );
            } else {
                let prefix_full = ts.guard_fail.replacen("{}", &g.violations.len().to_string(), 1);
                let prefix = prefix_full.strip_suffix("{}").unwrap_or(&prefix_full);
                line = line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(T12))
                        .text_color(fg)
                        .child(prefix.to_string()),
                );
                let sep = if matches!(self.lang, Lang::Zh) {
                    "、"
                } else {
                    ", "
                };
                for (i, v) in g.violations.iter().enumerate() {
                    if i > 0 {
                        line = line.child(
                            div()
                                .text_size(px(T12))
                                .text_color(fg)
                                .child(sep.to_string()),
                        );
                    }
                    let idx = self.violation_index(v.start);
                    line = if let Some(vi) = idx {
                        line.child(
                            div()
                                .id(sid(format!("sum-{}", v.start)))
                                .text_size(px(T12))
                                .text_color(fg)
                                .border_b_2()
                                .border_color(fg)
                                .child(v.raw.clone())
                                .on_hover(cx.listener(
                                    move |ws: &mut Workspace,
                                          hov: &bool,
                                          _w: &mut Window,
                                          cx: &mut Context<Workspace>| {
                                        ws.hovered_violation = if *hov {
                                            Some(vi)
                                        } else if ws.hovered_violation == Some(vi) {
                                            None
                                        } else {
                                            ws.hovered_violation
                                        };
                                        cx.notify();
                                    },
                                )),
                        )
                    } else {
                        line.child(
                            div()
                                .text_size(px(T12))
                                .text_color(fg)
                                .child(v.raw.clone()),
                        )
                    };
                }
            }
            sec = sec.child(line);
        }

        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .py_1()
            .text_size(px(T14))
            .line_height(px(22.0))
            .text_color(c(FG));

        let spans: &[NumSpan] = guardrail
            .as_ref()
            .map(|g| g.spans.as_slice())
            .unwrap_or(&[]);
        let mut span_idx = 0usize;
        let mut offset = 0usize;
        for line in inference.split('\n') {
            let line_start = offset;
            let line_end = offset + line.len();
            let mut row = div().flex().flex_row().flex_wrap().gap_0();
            let mut pos = line_start;
            while span_idx < spans.len() && spans[span_idx].end <= pos {
                span_idx += 1;
            }
            for span in &spans[span_idx.min(spans.len())..] {
                if span.start >= line_end {
                    break;
                }
                let s = span.start.max(pos);
                let e = span.end.min(line_end);
                if s > pos {
                    row =
                        row.child(div().child(line[pos - line_start..s - line_start].to_string()));
                }
                let raw = line[s - line_start..e - line_start].to_string();
                if span.ok {
                    row = row.child(div().text_color(c(CYAN)).child(raw));
                } else {
                    let idx = self.violation_index(span.start);
                    let hovered = self.hovered_violation == idx;
                    let mut flagged = div()
                        .id(sid(format!("vio-{}", span.start)))
                        .text_color(c(RED))
                        .border_b_2()
                        .border_color(c(RED))
                        .child(raw)
                        .on_hover(cx.listener(
                            move |ws: &mut Workspace,
                                  hov: &bool,
                                  _w: &mut Window,
                                  cx: &mut Context<Workspace>| {
                                ws.hovered_violation = if *hov {
                                    idx
                                } else if ws.hovered_violation == idx {
                                    None
                                } else {
                                    ws.hovered_violation
                                };
                                cx.notify();
                            },
                        ));
                    if hovered {
                        flagged = flagged.bg(c(RED_BG));
                    }
                    row = row.child(flagged);
                }
                pos = e;
            }
            if pos < line_end {
                row = row.child(div().child(line[pos - line_start..].to_string()));
            }
            body = body.child(row);
            offset = line_end + 1; // '\n'
        }

        sec = sec.child(body);

        // Hover strip: explanation for the flagged number (source tooltip, F4).
        if let (Some(g), Some(idx)) = (&self.guardrail, self.hovered_violation) {
            if let Some(v) = g.violations.iter().find(|v| v.start == idx) {
                sec = sec.child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(c(RED_BG))
                        .text_size(px(12.0))
                        .text_color(c(RED))
                        .child(match self.lang {
                            Lang::En => i18n::guardrail_hint_en(&v.raw),
                            Lang::Zh => v.hint.clone(),
                        }),
                );
            }
        }
        sec
    }

    fn violation_index(&self, start: usize) -> Option<usize> {
        self.guardrail
            .as_ref()
            .and_then(|g| g.violations.iter().position(|v| v.start == start))
    }

    fn section_check(&self, check: &str) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let mut sec = self.section_frame(ts.sec_check, c(FG3));
        sec = sec.child(
            div()
                .text_size(px(13.0))
                .text_color(c(DIM))
                .child(check.to_string()),
        );
        sec
    }

    // ------------------------------------------------------- evidence drawer

    pub fn render_evidence_panel(
        &mut self,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ts = i18n::strings(self.lang);
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .mt_2()
            .p_3()
            .rounded_sm()
            .bg(c(PANEL))
            .border_1()
            .border_color(c(BORDER));

        panel = panel.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(c(FG))
                        .child(ts.evidence_title),
                )
                .child(self.button(
                    ts.evidence_hide,
                    Btn::Ghost,
                    Some("\u{E8BB}"),
                    _w,
                    cx,
                    |ws, _e, _w, cx| {
                        ws.show_evidence = false;
                        ws.hovered_violation = None;
                        cx.notify();
                    },
                )),
        );

        if let Some(ev) = &self.evidence {
            panel = panel
                .child(self.kv("event_id", &ev.event_id))
                .child(self.kv("locator", &ev.locator))
                .child(self.kv("fetched_at", &ev.fetched_at))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(c(DIM))
                                .child(ts.evidence_excerpt),
                        )
                        .child(
                            div()
                                .p_2()
                                .rounded_sm()
                                .bg(c(PANEL2))
                                .text_size(px(12.0))
                                .text_color(c(FG))
                                .font_family(FONT_MONO)
                                .child(ev.excerpt.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(c(ACCENT))
                                .font_family(FONT_MONO)
                                .child(ev.source_url.clone()),
                        ),
                );
        }
        if let Some(ctx) = &self.context {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(c(DIM))
                            .child(ts.evidence_announcement),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(c(FG))
                            .child(ctx.announcement.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(c(DIM))
                            .child(ctx.announcement.summary.clone()),
                    ),
            );
        }
        if let Some(m) = &self.methodology {
            let mut mdiv = div().flex().flex_col().gap_1().child(
                div()
                    .text_size(px(11.0))
                    .text_color(c(DIM))
                    .child(ts.evidence_methodology),
            );
            for step in &m.pipeline {
                mdiv = mdiv.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(c(DIM))
                        .child(format!("· {step}")),
                );
            }
            mdiv = mdiv.child(
                div()
                    .text_size(px(11.0))
                    .font_family(FONT_MONO)
                    .text_color(c(FG3))
                    .child(format!(
                        "tolerance {}% · pp {} · checked_at {}",
                        m.validation.tolerance_pct,
                        m.validation.pp_tolerance,
                        m.validation.checked_at
                    )),
            );
            panel = panel.child(mdiv);
        }
        panel
    }

    fn kv(&self, k: &str, v: &str) -> gpui::Div {
        div()
            .flex()
            .flex_row()
            .gap_2()
            .text_size(px(12.0))
            .child(
                div()
                    .min_w(px(90.0))
                    .text_color(c(DIM))
                    .child(k.to_string()),
            )
            .child(
                div()
                    .font_family(FONT_MONO)
                    .text_color(c(FG))
                    .child(v.to_string()),
            )
    }

    // ---------------------------------------------------------- review modal

    pub fn render_review_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let Some(flow) = &mut self.review else {
            return div();
        };
        let verdict = flow.verdict;
        // Mirror the submit handler: the key is derived from the task the detail
        // belongs to, not from whatever row is still marked selected.
        let task_id = self
            .detail
            .as_ref()
            .map(|d| d.task_id.clone())
            .unwrap_or_default();
        let version = flow.brief_version;
        let idempotency_key = review_idempotency_key(&task_id, version);

        let tabs = [Verdict::Approve, Verdict::ApproveAsRevised, Verdict::Reject]
            .into_iter()
            .map(|v| {
                let selected = flow.verdict == v;
                let dot = match v {
                    Verdict::Approve => c(GREEN),
                    Verdict::ApproveAsRevised => c(ACCENT),
                    Verdict::Reject => c(RED),
                };
                // One selection treatment for every segmented control in the
                // app (accent); the dot keeps the verdict's semantics.
                let mut el = div()
                    .id(sid(format!("verdict-{}", v.as_str())))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .px_2p5()
                    .py_1()
                    .rounded_sm()
                    .text_size(px(T13))
                    .cursor_pointer();
                el = if selected {
                    el.bg(c(ACCENT_DIM)).text_color(c(ACCENT))
                } else {
                    el.text_color(c(FG2)).hover(|s| s.bg(c(RAISED)).text_color(c(FG)))
                };
                el.child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(dot))
                    .child(match v {
                        Verdict::Approve => ts.approve,
                        Verdict::ApproveAsRevised => ts.approve_as_revised,
                        Verdict::Reject => ts.reject,
                    })
                    .on_click(cx.listener(move |ws, _e: &ClickEvent, _w, cx| {
                        if let Some(flow) = &mut ws.review {
                            flow.verdict = v;
                            cx.notify();
                        }
                    }))
            })
            .collect::<Vec<_>>();

        let mut body = div().flex().flex_col().gap_3();
        body = body.child(div().flex().flex_row().gap_1().children(tabs));

        // Comment box.
        let comment_label = div()
            .text_size(px(12.0))
            .text_color(c(DIM))
            .child(ts.comment_label);
        body = body.child(comment_label).child(flow.comment.clone());

        // Revise editor + live diff.
        if verdict == Verdict::ApproveAsRevised {
            // open_review always creates the editor; bail out of rendering
            // rather than panic if the invariant is ever broken.
            let Some(editor) = flow.editor.clone() else {
                return div();
            };
            let original = flow.original_inference.clone();
            let current = editor.read(cx).text().to_string();
            let diff = diff_lines(&original, &current);

            let mut diff_rows = div()
                .flex()
                .flex_col()
                .rounded_sm()
                .border_1()
                .border_color(c(BORDER))
                .py_1()
                .max_h(px(260.0))
                .id("diff-scroll")
                .overflow_y_scroll();
            for d in &diff {
                let (bg, mark, txt_color) = match d.op {
                    DiffOp::Same => (c(PANEL), " ", c(DIM)),
                    DiffOp::Insert => (c(GREEN_BG), "+", c(GREEN)),
                    DiffOp::Delete => (c(RED_BG), "-", c(RED)),
                };
                diff_rows = diff_rows.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .px_1()
                        .bg(bg)
                        .font_family(FONT_MONO)
                        .text_size(px(12.0))
                        .text_color(txt_color)
                        .child(div().min_w(px(14.0)).child(mark.to_string()))
                        .child(div().flex_1().child(d.text.clone())),
                );
            }

            let changed = diff.iter().filter(|d| d.op != DiffOp::Same).count();
            let summary = if changed == 0 {
                ts.diff_same.to_string()
            } else {
                ts.diff_changed.replacen("{}", &changed.to_string(), 1)
            };
            body = body
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(c(DIM))
                        .child(ts.revise_label),
                )
                .child(editor)
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(if changed == 0 { c(DIM) } else { c(ACCENT) })
                        .child(summary),
                )
                .child(diff_rows);
        }

        let error_line = flow.error.clone().map(|e| {
            div()
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(c(RED_BG))
                .text_size(px(12.0))
                .text_color(c(RED))
                .child(e)
        });

        let submitting = flow.submitting;
        let footer = div()
            .flex()
            .flex_row()
            .gap_2()
            .items_center()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(11.0))
                            .text_color(c(DIM))
                            .font_family(FONT_MONO)
                            .truncate()
                            .child(format!("idempotency_key = {idempotency_key}")),
                    )
                    // Shortcuts as keycaps, not glowing text.
                    .child(keycap("Esc"))
                    .child(
                        div()
                            .text_size(px(T11))
                            .text_color(c(DIM))
                            .child(ts.hint_cancel),
                    )
                    .child(keycap("Ctrl+Enter"))
                    .child(
                        div()
                            .text_size(px(T11))
                            .text_color(c(DIM))
                            .child(ts.hint_submit),
                    ),
            )
            .child(self.button(
                ts.cancel,
                Btn::Secondary,
                None,
                window,
                cx,
                |ws, _e, _w, cx| {
                    ws.review = None;
                    ws.hovered_violation = None;
                    cx.notify();
                },
            ))
            .child(if submitting {
                div()
                    .px_3()
                    .py_1()
                    .text_size(px(13.0))
                    .text_color(c(DIM))
                    .child(ts.submitting)
                    .into_any_element()
            } else {
                self.button(
                    ts.submit,
                    Btn::Primary,
                    None,
                    window,
                    cx,
                    |ws, _e, _w, cx| {
                        ws.submit_review(cx);
                    },
                )
                .into_any_element()
            });

        let avail = (window.viewport_size().width / window.scale_factor()) - px(220.0 + 16.0 * 2.0);
        let modal_ml = ((avail - px(880.0)) / 2.0).max(px(0.0));
        div().flex().flex_col().child(
            div()
                .ml(modal_ml)
                .w(px(880.0))
                .flex()
                .flex_col()
                .gap_3()
                .rounded_lg()
                .bg(c(PANEL))
                .border_1()
                .border_color(c(BORDER))
                .p_4()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(16.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(c(FG))
                                .child(ts.review_title),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(c(DIM))
                                .font_family(FONT_MONO)
                                .child(task_id.clone()),
                        ),
                )
                .child(body)
                .children(error_line)
                .child(footer),
        )
    }

    /// Split-view reader for the Brief library: renders the vault brief's
    /// markdown beside the list — identity, sections, guardrail verdict —
    /// without leaving the view.
    pub fn render_library_reader(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let ts = i18n::strings(self.lang);
        let Some(brief) = self.library_reader.clone() else {
            return div();
        };
        let fm = &brief.frontmatter;
        let close = self.button("", Btn::Ghost, Some("\u{E8BB}"), window, cx, |ws, _e, _w, cx| {
            ws.library_reader = None;
            cx.notify();
        });

        // Grows to fill everything right of the list.
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(420.0))
            .h_full()
            .bg(c(SURFACE))
            .border_l_1()
            .border_color(c(HAIRLINE));

        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .id("reader-scroll")
            .overflow_y_scroll()
            .overflow_x_hidden()
            .p_4()
            .pr_5()
            .gap_2();

        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .child(div().flex_1())
                .child(close),
        );
        body = body.child(
            div()
                .text_size(px(T15))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(c(FG))
                .truncate()
                .child(brief.title.clone()),
        );
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(Badge::new(
                    i18n::status_label(self.lang, &fm.status),
                    status_tone(&fm.status),
                ))
                .child(self.cutoff_badge(&fm.data_cutoff, &fm.source_version)),
        );
        if !fm.reviewer.is_empty() {
            body = body.child(
                div()
                    .text_size(px(11.0))
                    .font_family(FONT_MONO)
                    .text_color(c(FG2))
                    .child(format!("{} {}", ts.reader_reviewer, fm.reviewer)),
            );
        }

        // FACT as bullet lines, links in mono accent — same treatment as the
        // full detail view.
        if let Some(fact) = brief.section(SectionKind::Fact) {
            let lines: Vec<String> = fact
                .body
                .lines()
                .map(|l| l.trim().trim_start_matches("- ").trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            body = body.child(self.section_fact(&lines));
        }

        // CALCULATION as the metric grid.
        let rows: Vec<(String, String)> = brief
            .calc_values()
            .iter()
            .map(|(k, v)| (k.clone(), format!("{}", v)))
            .collect();
        if !rows.is_empty() {
            let mut sec = self.section_frame(ts.sec_calc, c(FG3));
            sec = sec.child(self.metric_grid(rows));
            body = body.child(sec);
        }

        // INFERENCE as plain text — the CHECK section below carries the
        // review outcome; the reader has no server-side evidence to re-run
        // the guardrail against, so it stays quiet here.
        if let Some(inference) = brief.section(SectionKind::Inference) {
            body = body.child(self.section_inference(&inference.body, &None, cx));
        }

        if let Some(check) = brief.section(SectionKind::Check) {
            body = body.child(self.section_check(&check.body));
        }

        // The vault path lives here, not in the list rows.
        body = body.child(
            div()
                .text_size(px(11.0))
                .font_family(FONT_MONO)
                .text_color(c(FG3))
                .truncate()
                .child(brief.path.display().to_string()),
        );

        body = body.child(
            div().flex().flex_row().pt_1().child(self.button(
                ts.reader_open_review,
                Btn::Primary,
                None,
                window,
                cx,
                |ws, _e, _w, cx| {
                    let tid = ws
                        .library_reader
                        .as_ref()
                        .map(|b| b.frontmatter.task_id.clone())
                        .unwrap_or_default();
                    ws.library_reader = None;
                    ws.view = crate::workspace::ViewKind::Inbox;
                    ws.open_task(&tid, cx);
                },
            )),
        );

        panel.child(body)
    }
}

fn fmt_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
