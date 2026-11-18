//! Duplicate / restatement detection across briefs (advisory).
//!
//! Two briefs can describe the same event. When they do, a reviewer approving
//! the second should know that the first exists and whether the numbers agree —
//! a restated figure that moved is the signal, and it is invisible in either
//! brief read alone.
//!
//! ## What counts as a duplicate
//!
//! Three signals, strongest first, and they are deliberately independent of the
//! brief's own prose (a model can restate a figure while rewriting every
//! sentence):
//!
//! 1. **Same event** — `event_ref` in the brief frontmatter (or `event_id` on the
//!    evidence pack) is identical. This is the only signal that identifies an
//!    event exactly; the others are heuristics.
//! 2. **Same company and period** — `object_id` + `period` equal. Catches the
//!    case where the service emitted two tasks for one filing under different
//!    event ids (the jizhi / 德冠新材 fixture pair in this repo does exactly
//!    that).
//! 3. **Same figure set** — the CALCULATION value maps agree on a large
//!    fraction of keys within tolerance, regardless of company/period. Catches
//!    a re-run that changed only the narrative.
//!
//! ## What it does not do
//!
//! It does not merge, delete, approve or reject. A duplicate is reported, never
//! resolved: which brief is authoritative is a reviewer's call, and the audit
//! trail records that call.

use std::collections::BTreeMap;

use crate::guardrail::{within_tolerance, GuardrailConfig};

/// The identity of a brief, extracted from frontmatter or the task payload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BriefIdentity {
    pub task_id: String,
    pub event_ref: String,
    pub object_id: String,
    pub period: String,
    /// CALCULATION values (tool output), used for figure-set comparison.
    pub calc: BTreeMap<String, f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DuplicateKind {
    /// Same `event_ref`: one event, two briefs.
    SameEvent,
    /// Same company and period, different event ids.
    SameCompanyPeriod,
    /// Different event, but the computed figures agree.
    SameFigures,
}

impl DuplicateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DuplicateKind::SameEvent => "same-event",
            DuplicateKind::SameCompanyPeriod => "same-company-period",
            DuplicateKind::SameFigures => "same-figures",
        }
    }
}

/// One detected duplicate of the brief under review.
#[derive(Clone, Debug)]
pub struct Duplicate {
    pub kind: DuplicateKind,
    pub other_task_id: String,
    pub other_object_id: String,
    pub other_period: String,
    /// Figures that differ between the two briefs: `(key, mine, theirs)`.
    pub divergent: Vec<(String, f64, f64)>,
    /// 0.0–1.0. 1.0 for `SameEvent` (identity, not a guess).
    pub confidence: f64,
}

impl Duplicate {
    /// True when the two briefs disagree on at least one computed figure. This
    /// is the case that matters: an identical restatement is noise, a restated
    /// figure that moved is a contradiction.
    pub fn conflicts(&self) -> bool {
        !self.divergent.is_empty()
    }
}

/// Fraction of shared CALC keys whose values agree within tolerance.
fn figure_agreement(a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>, cfg: &GuardrailConfig) -> f64 {
    let shared: Vec<(&String, &f64)> = a.iter().filter(|(k, _)| b.contains_key(*k)).collect();
    if shared.is_empty() {
        return 0.0;
    }
    let agree = shared
        .iter()
        .filter(|(k, v)| {
            let other = b[*k];
            // Percentages carry the pp floor; amounts do not. The key name is
            // the only signal available here, so it decides the regime.
            within_tolerance(**v, other, k.contains("pct") || k.contains("rate"), cfg)
        })
        .count();
    agree as f64 / shared.len() as f64
}

/// Find briefs that duplicate `me`. `others` is every brief the caller knows
/// about, including `me` itself (which is skipped).
pub fn find_duplicates(
    me: &BriefIdentity,
    others: &[BriefIdentity],
    cfg: &GuardrailConfig,
) -> Vec<Duplicate> {
    let mut out = Vec::new();
    for other in others {
        if other.task_id == me.task_id {
            continue;
        }
        let kind = if !me.event_ref.is_empty() && me.event_ref == other.event_ref {
            DuplicateKind::SameEvent
        } else if !me.object_id.is_empty()
            && me.object_id == other.object_id
            && !me.period.is_empty()
            && me.period == other.period
        {
            DuplicateKind::SameCompanyPeriod
        } else if figure_agreement(&me.calc, &other.calc, cfg) >= 0.8 {
            DuplicateKind::SameFigures
        } else {
            continue;
        };

        let mut divergent = Vec::new();
        for (key, mine) in &me.calc {
            let Some(theirs) = other.calc.get(key) else {
                continue;
            };
            let is_pct = key.contains("pct") || key.contains("rate");
            if !within_tolerance(*mine, *theirs, is_pct, cfg) {
                divergent.push((key.clone(), *mine, *theirs));
            }
        }
        divergent.sort_by(|a, b| a.0.cmp(&b.0));

        let confidence = match kind {
            DuplicateKind::SameEvent => 1.0,
            DuplicateKind::SameCompanyPeriod => 0.9,
            DuplicateKind::SameFigures => figure_agreement(&me.calc, &other.calc, cfg),
        };

        out.push(Duplicate {
            kind,
            other_task_id: other.task_id.clone(),
            other_object_id: other.object_id.clone(),
            other_period: other.period.clone(),
            divergent,
            confidence,
        });
    }
    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.kind.cmp(&b.kind))
    });
    out
}

/// One-line description for the reviewer UI, English-side. The app localises the
/// label; this keeps the shape of the sentence in one place.
pub fn describe(d: &Duplicate) -> String {
    let base = match d.kind {
        DuplicateKind::SameEvent => "same event",
        DuplicateKind::SameCompanyPeriod => "same company and period",
        DuplicateKind::SameFigures => "identical computed figures",
    };
    if d.divergent.is_empty() {
        format!("{base}, figures agree")
    } else {
        let keys: Vec<&str> = d.divergent.iter().map(|(k, _, _)| k.as_str()).collect();
        format!("{base}, figures differ: {}", keys.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> GuardrailConfig {
        GuardrailConfig::default()
    }

    fn id(task: &str, event: &str, object: &str, period: &str) -> BriefIdentity {
        BriefIdentity {
            task_id: task.into(),
            event_ref: event.into(),
            object_id: object.into(),
            period: period.into(),
            calc: BTreeMap::new(),
        }
    }

    fn with_calc(mut b: BriefIdentity, pairs: &[(&str, f64)]) -> BriefIdentity {
        for (k, v) in pairs {
            b.calc.insert((*k).into(), *v);
        }
        b
    }

    #[test]
    fn same_event_is_a_duplicate_at_full_confidence() {
        let a = id("RT-1", "EV-1", "S300806", "2026Q3");
        let b = id("RT-2", "EV-1", "S300806", "2026Q3");
        let d = find_duplicates(&a, &[a.clone(), b], &cfg());
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, DuplicateKind::SameEvent);
        assert_eq!(d[0].confidence, 1.0);
    }

    #[test]
    fn itself_is_never_a_duplicate() {
        let a = id("RT-1", "EV-1", "S300806", "2026Q3");
        assert!(find_duplicates(&a, &[a.clone()], &cfg()).is_empty());
    }

    /// The repo's own fixture pair: 德冠新材 filed under two event ids.
    #[test]
    fn same_company_and_period_with_different_event_ids() {
        let a = id("RT-1", "EV-A", "S300566", "2026Q3");
        let b = id("RT-2", "EV-B", "S300566", "2026Q3");
        let d = find_duplicates(&a, &[b], &cfg());
        assert_eq!(d[0].kind, DuplicateKind::SameCompanyPeriod);
    }

    #[test]
    fn different_company_same_period_is_not_a_duplicate() {
        let a = id("RT-1", "EV-A", "S300566", "2026Q3");
        let b = id("RT-2", "EV-B", "S002859", "2026Q3");
        assert!(find_duplicates(&a, &[b], &cfg()).is_empty());
    }

    #[test]
    fn identical_figures_detect_a_rerun() {
        let pairs = [("revenue", 24.83), ("net_profit", 0.669378)];
        let a = with_calc(id("RT-1", "EV-A", "S300806", "2026Q3"), &pairs);
        let b = with_calc(id("RT-2", "EV-B", "S999999", "2026Q4"), &pairs);
        let d = find_duplicates(&a, &[b], &cfg());
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, DuplicateKind::SameFigures);
        assert!(!d[0].conflicts());
    }

    /// A small move is *not* a conflict — the tolerance that governs the
    /// guardrail governs restatement comparison too, so the two cannot disagree
    /// about whether two figures are "the same number".
    #[test]
    fn small_move_inside_tolerance_is_not_a_conflict() {
        let a = with_calc(
            id("RT-1", "EV-A", "S300806", "2026Q3"),
            &[("revenue", 24.83), ("net_profit", 0.669378)],
        );
        let b = with_calc(
            id("RT-2", "EV-B", "S300806", "2026Q3"),
            &[("revenue", 24.91), ("net_profit", 0.669378)],
        );
        let d = find_duplicates(&a, &[b], &cfg());
        assert_eq!(d[0].kind, DuplicateKind::SameCompanyPeriod);
        assert!(!d[0].conflicts(), "24.83 vs 24.91 is 0.32% — inside 1%");
    }

    #[test]
    fn divergence_beyond_tolerance_is_reported() {
        let a = with_calc(
            id("RT-1", "EV-A", "S300806", "2026Q3"),
            &[("revenue", 24.83)],
        );
        let b = with_calc(
            id("RT-2", "EV-B", "S300806", "2026Q3"),
            &[("revenue", 26.50)],
        );
        let d = find_duplicates(&a, &[b], &cfg());
        assert!(d[0].conflicts());
        assert_eq!(d[0].divergent[0].0, "revenue");
        assert!(describe(&d[0]).contains("revenue"));
    }

    #[test]
    fn agreement_ignores_keys_the_other_brief_lacks() {
        let a = with_calc(
            id("RT-1", "EV-A", "S1", "2026Q3"),
            &[("revenue", 24.83), ("ocf", -0.74839)],
        );
        let b = with_calc(id("RT-2", "EV-B", "S2", "2026Q3"), &[("revenue", 24.83)]);
        let d = find_duplicates(&a, &[b], &cfg());
        // One shared key, agreeing → 100% agreement → SameFigures.
        assert_eq!(d[0].kind, DuplicateKind::SameFigures);
        assert!(!d[0].conflicts(), "unshared keys are not conflicts");
    }

    #[test]
    fn empty_calc_never_matches_on_figures() {
        let a = id("RT-1", "EV-A", "S1", "2026Q3");
        let b = id("RT-2", "EV-B", "S2", "2026Q4");
        assert!(find_duplicates(&a, &[b], &cfg()).is_empty());
    }

    #[test]
    fn duplicates_are_ordered_by_confidence() {
        let me = with_calc(
            id("RT-1", "EV-A", "S300806", "2026Q3"),
            &[("revenue", 24.83)],
        );
        let same_event = id("RT-2", "EV-A", "S300806", "2026Q3");
        let same_figs = with_calc(
            id("RT-3", "EV-Z", "S000001", "2026Q1"),
            &[("revenue", 24.83)],
        );
        let d = find_duplicates(&me, &[same_figs, same_event], &cfg());
        assert_eq!(d[0].kind, DuplicateKind::SameEvent);
    }

    #[test]
    fn tiny_float_noise_does_not_create_a_conflict() {
        // The guardrail trims values to 1e-6 before comparing; a figure that
        // round-trips through JSON must not read as "moved".
        let a = with_calc(
            id("RT-1", "EV-A", "S300806", "2026Q3"),
            &[("revenue", 24.830000001)],
        );
        let b = with_calc(
            id("RT-2", "EV-B", "S300806", "2026Q3"),
            &[("revenue", 24.83)],
        );
        let d = find_duplicates(&a, &[b], &cfg());
        assert!(!d[0].conflicts());
    }
}
