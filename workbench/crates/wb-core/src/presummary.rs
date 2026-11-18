//! Pre-review summarisation (advisory): what changed since the last version.
//!
//! A reviewer opening v2 of a brief should not have to diff it by eye against v1
//! to find out what moved. This module produces the one-paragraph answer, from
//! the two brief texts and their CALCULATION maps.
//!
//! ## Why there is no LLM here
//!
//! A summary written by a model is a second model-generated text, and every
//! number in it would need the guardrail run over it in turn — a summary of a
//! summary, with no receipt. So the summary is assembled from the diff and the
//! CALC deltas, both deterministic, and it can state figures because each one is
//! copied from a section that already passed (or already failed) the guardrail.
//! The reviewer's chat panel (F14) is where free-form explanation belongs; this
//! is the structured headline.
//!
//! ## What it reports
//!
//! - which sections changed, and whether the change is cosmetic or numeric
//! - every CALC figure that moved, with both values
//! - every number that appears in the new INFERENCE and did not appear in the
//!   old one (the numbers most likely to be newly invented)
//! - the guardrail verdict delta, when both versions have one

use std::collections::BTreeMap;

use crate::diff::{diff_lines, DiffOp};
use crate::guardrail::number_regex;

/// Which section of the brief a change landed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionId {
    Fact,
    Calculation,
    Inference,
    Check,
}

impl SectionId {
    pub fn label(self) -> &'static str {
        match self {
            SectionId::Fact => "FACT",
            SectionId::Calculation => "CALCULATION",
            SectionId::Inference => "INFERENCE",
            SectionId::Check => "CHECK",
        }
    }
}

/// One figure that moved between versions.
#[derive(Clone, Debug, PartialEq)]
pub struct CalcDelta {
    pub key: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
}

impl CalcDelta {
    /// True when the key exists in only one version — a figure added or dropped,
    /// which is a different kind of change from a figure that moved.
    pub fn is_added_or_removed(&self) -> bool {
        self.before.is_none() || self.after.is_none()
    }
}

/// A number present in the new inference but absent from the old one.
#[derive(Clone, Debug, PartialEq)]
pub struct NewNumber {
    pub raw: String,
    pub value: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    /// Sections whose text differs, in brief order.
    pub changed: Vec<SectionId>,
    /// Figures that moved, added or dropped.
    pub calc_deltas: Vec<CalcDelta>,
    /// Numbers new to the INFERENCE section.
    pub new_numbers: Vec<NewNumber>,
    /// Numbers removed from the INFERENCE section.
    pub dropped_numbers: Vec<NewNumber>,
    /// `Some((before, after))` when both versions carry a guardrail verdict.
    pub verdict_change: Option<(bool, bool)>,
    /// Lines added / removed across all sections.
    pub lines_added: usize,
    pub lines_removed: usize,
}

impl Summary {
    /// Nothing a reviewer needs to read: no text change, no figure change.
    pub fn is_trivial(&self) -> bool {
        self.changed.is_empty()
            && self.calc_deltas.is_empty()
            && self.new_numbers.is_empty()
            && self.dropped_numbers.is_empty()
            && self.lines_added == 0
            && self.lines_removed == 0
            // A verdict flip is never trivial, even when the text is identical:
            // the same words passing or failing the guardrail is exactly what a
            // reviewer needs to know.
            && self.verdict_change.is_none()
    }

    /// True when the revision changed a figure or introduced a number — the
    /// changes that need checking, as opposed to rewording.
    pub fn touches_numbers(&self) -> bool {
        !self.calc_deltas.is_empty() || !self.new_numbers.is_empty()
    }
}

fn numbers_in(text: &str) -> Vec<NewNumber> {
    let mut out: Vec<NewNumber> = Vec::new();
    for m in number_regex().find_iter(text) {
        let raw = m.as_str().replace(',', "");
        let Ok(value) = raw.parse::<f64>() else { continue };
        if !out.iter().any(|n| n.value == value) {
            out.push(NewNumber {
                raw: m.as_str().to_string(),
                value,
            });
        }
    }
    out
}

/// Compare two versions of a brief.
///
/// `sections` are the four bodies in order — pass the same shape for both
/// versions. `calc` maps are the parsed CALCULATION values.
pub fn summarize(
    before: &[(SectionId, String)],
    after: &[(SectionId, String)],
    before_calc: &BTreeMap<String, f64>,
    after_calc: &BTreeMap<String, f64>,
) -> Summary {
    let mut s = Summary::default();

    for (id, new_text) in after {
        let old_text = before
            .iter()
            .find(|(bid, _)| bid == id)
            .map(|(_, t)| t.as_str())
            .unwrap_or("");
        if old_text == new_text {
            continue;
        }
        s.changed.push(*id);
        for line in diff_lines(old_text, new_text) {
            match line.op {
                DiffOp::Insert => s.lines_added += 1,
                DiffOp::Delete => s.lines_removed += 1,
                DiffOp::Same => {}
            }
        }
    }

    // CALC deltas: union of keys, ordered.
    let mut keys: Vec<&String> = before_calc.keys().chain(after_calc.keys()).collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let b = before_calc.get(key).copied();
        let a = after_calc.get(key).copied();
        if b != a {
            s.calc_deltas.push(CalcDelta {
                key: key.clone(),
                before: b,
                after: a,
            });
        }
    }

    // Numbers new to the inference: the model's own arithmetic lives here.
    let old_inf = before
        .iter()
        .find(|(id, _)| *id == SectionId::Inference)
        .map(|(_, t)| t.as_str())
        .unwrap_or("");
    let new_inf = after
        .iter()
        .find(|(id, _)| *id == SectionId::Inference)
        .map(|(_, t)| t.as_str())
        .unwrap_or("");
    let old_nums = numbers_in(old_inf);
    let new_nums = numbers_in(new_inf);
    s.new_numbers = new_nums
        .iter()
        .filter(|n| !old_nums.iter().any(|o| o.value == n.value))
        .cloned()
        .collect();
    s.dropped_numbers = old_nums
        .iter()
        .filter(|o| !new_nums.iter().any(|n| n.value == o.value))
        .cloned()
        .collect();

    s
}

/// Record the guardrail verdict for both versions so the summary can report a
/// fix or a regression.
pub fn with_verdicts(mut s: Summary, before_ok: Option<bool>, after_ok: Option<bool>) -> Summary {
    if let (Some(b), Some(a)) = (before_ok, after_ok) {
        if b != a {
            s.verdict_change = Some((b, a));
        }
    }
    s
}

/// One-line English headline for the UI. The app localises; the sentence shape
/// lives here so it stays consistent with the tests.
pub fn headline(s: &Summary) -> String {
    if s.is_trivial() {
        return "no change".into();
    }
    let mut parts: Vec<String> = Vec::new();
    if !s.changed.is_empty() {
        let names: Vec<&str> = s.changed.iter().map(|c| c.label()).collect();
        parts.push(format!("sections: {}", names.join(", ")));
    }
    if !s.calc_deltas.is_empty() {
        parts.push(format!("{} figure(s) changed", s.calc_deltas.len()));
    }
    if !s.new_numbers.is_empty() {
        let vals: Vec<&str> = s.new_numbers.iter().map(|n| n.raw.as_str()).collect();
        parts.push(format!("new numbers: {}", vals.join(", ")));
    }
    if let Some((b, a)) = s.verdict_change {
        parts.push(match (b, a) {
            (false, true) => "guardrail: violation fixed".into(),
            (true, false) => "guardrail: NEW violation".to_string(),
            _ => format!("guardrail: {b} → {a}"),
        });
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| ((*k).into(), *v)).collect()
    }

    fn sections(inf: &str, calc_body: &str) -> Vec<(SectionId, String)> {
        vec![
            (SectionId::Fact, "斯迪克：2026年三季度报告".into()),
            (SectionId::Calculation, calc_body.into()),
            (SectionId::Inference, inf.into()),
            (SectionId::Check, "数字校验通过".into()),
        ]
    }

    #[test]
    fn identical_versions_are_trivial() {
        let s = sections("营收24.83亿元。", "revenue = 24.83");
        let sum = summarize(&s, &s, &calc(&[("revenue", 24.83)]), &calc(&[("revenue", 24.83)]));
        assert!(sum.is_trivial());
        assert_eq!(headline(&sum), "no change");
    }

    /// The 斯迪克 revise flow: the reviewer deleted the invented 1652万.
    #[test]
    fn removing_the_invented_number_is_reported() {
        let before = sections("扣非低于归母约1652万元，营收24.83亿元。", "revenue = 24.83");
        let after = sections("营收24.83亿元。", "revenue = 24.83");
        let c = calc(&[("revenue", 24.83)]);
        let sum = summarize(&before, &after, &c, &c);
        assert!(sum.changed.contains(&SectionId::Inference));
        assert!(sum.new_numbers.is_empty(), "nothing new was introduced");
        assert_eq!(sum.dropped_numbers.len(), 1);
        assert_eq!(sum.dropped_numbers[0].value, 1652.0);
        assert!(!sum.touches_numbers(), "a dropped number is not a new figure");
    }

    #[test]
    fn a_new_number_in_the_inference_is_surfaced() {
        let before = sections("营收24.83亿元。", "revenue = 24.83");
        let after = sections("营收24.83亿元，净利率约2.71%。", "revenue = 24.83");
        let c = calc(&[("revenue", 24.83)]);
        let sum = summarize(&before, &after, &c, &c);
        assert_eq!(sum.new_numbers.len(), 1);
        assert_eq!(sum.new_numbers[0].raw, "2.71");
        assert!(sum.touches_numbers());
        assert!(headline(&sum).contains("2.71"));
    }

    #[test]
    fn moved_calc_figure_is_a_delta() {
        let s = sections("营收24.83亿元。", "revenue = 24.83");
        let sum = summarize(
            &s,
            &s,
            &calc(&[("revenue", 24.83)]),
            &calc(&[("revenue", 26.50)]),
        );
        assert_eq!(sum.calc_deltas.len(), 1);
        assert_eq!(sum.calc_deltas[0].before, Some(24.83));
        assert_eq!(sum.calc_deltas[0].after, Some(26.50));
        assert!(!sum.calc_deltas[0].is_added_or_removed());
    }

    #[test]
    fn added_calc_key_is_flagged_as_added() {
        let s = sections("营收24.83亿元。", "revenue = 24.83");
        let sum = summarize(
            &s,
            &s,
            &calc(&[("revenue", 24.83)]),
            &calc(&[("revenue", 24.83), ("net_margin_pct", 2.7)]),
        );
        assert_eq!(sum.calc_deltas.len(), 1);
        assert!(sum.calc_deltas[0].is_added_or_removed());
        assert_eq!(sum.calc_deltas[0].key, "net_margin_pct");
    }

    #[test]
    fn rewording_alone_does_not_touch_numbers() {
        let before = sections("营收同比增长，盈利改善。", "");
        let after = sections("营业收入同比增长，盈利能力有所改善。", "");
        let sum = summarize(&before, &after, &calc(&[]), &calc(&[]));
        assert!(!sum.is_trivial(), "the text did change");
        assert!(!sum.touches_numbers(), "but no figure moved");
    }

    #[test]
    fn verdict_fix_is_reported() {
        let s = sections("营收24.83亿元。", "");
        let sum = with_verdicts(summarize(&s, &s, &calc(&[]), &calc(&[])), Some(false), Some(true));
        assert_eq!(sum.verdict_change, Some((false, true)));
        assert!(headline(&sum).contains("violation fixed"));
    }

    #[test]
    fn new_violation_is_reported_loudly() {
        let s = sections("营收24.83亿元。", "");
        let sum = with_verdicts(summarize(&s, &s, &calc(&[]), &calc(&[])), Some(true), Some(false));
        assert!(headline(&sum).contains("NEW violation"));
    }

    #[test]
    fn missing_section_on_one_side_is_not_a_panic() {
        let before: Vec<(SectionId, String)> = vec![(SectionId::Inference, "旧".into())];
        let after = sections("新 24.83", "");
        let sum = summarize(&before, &after, &calc(&[]), &calc(&[]));
        assert!(!sum.changed.is_empty());
    }
}
