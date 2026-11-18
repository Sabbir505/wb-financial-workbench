//! Structural accounting checks (F4 layer 2): deterministic re-computation of
//! derived indicators from the CALCULATION section's own numbers.
//!
//! The lexical guardrail (`guardrail`) verifies *traceability* — every number in
//! the model text must appear in evidence. This module verifies *coherence* —
//! the numbers in the tool output must satisfy the accounting identities they
//! claim. Pure arithmetic on the brief's own values, so it can never
//! hallucinate: a flag here is a real inconsistency, not a model opinion.
//!
//! Rules (aliases in parentheses):
//! - margin-consistency:  net_margin_pct ≈ net_profit / revenue × 100
//! - deducted-margin:     deducted_margin_pct ≈ net_profit_deducted / revenue × 100
//! - revenue-yoy:         yoy_first (revenue_yoy_pct) ≈ (revenue / revenue_prior − 1) × 100
//! - profit-yoy:          net_profit_yoy_pct ≈ (net_profit / net_profit_prior − 1) × 100
//! - margin-range:        any *_margin_pct ∈ [−100, 100]（净利润不可能超过营收）
//! - balance-identity:    assets = liabilities + equity（资产负债表恒等式）
//!
//! Tolerances reuse the guardrail's: 1% relative, 0.02pp absolute floor.

use crate::guardrail::GuardrailConfig;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct StructuralViolation {
    /// Stable rule id, e.g. "margin-consistency" — for the UI and analytics.
    pub rule: &'static str,
    /// Human-facing explanation with expected vs stated values.
    pub detail: String,
    pub expected: f64,
    pub stated: f64,
}

#[derive(Clone, Debug, Default)]
pub struct StructuralReport {
    /// Rule ids that actually ran (had all their inputs present).
    pub checked: Vec<&'static str>,
    pub violations: Vec<StructuralViolation>,
}

impl StructuralReport {
    pub fn ok(&self) -> bool {
        self.violations.is_empty()
    }
}

fn get(calc: &BTreeMap<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| calc.get(*k).and_then(|v| v.as_f64()))
}

fn within(actual: f64, expected: f64, cfg: &GuardrailConfig) -> bool {
    let diff = (actual - expected).abs();
    diff <= (expected.abs() * cfg.tolerance_pct / 100.0).max(cfg.pp_tolerance)
}

fn fmt(v: f64) -> String {
    crate::guardrail::trim_float(v).to_string()
}

/// One ratio rule: derived key(s) must equal (a/b + offset) × scale.
fn check_ratio(
    calc: &BTreeMap<String, Value>,
    rule: &'static str,
    derived: &[&str],
    num: &[&str],
    den: &[&str],
    scale: f64,
    offset: f64,
    cfg: &GuardrailConfig,
    out: &mut StructuralReport,
) {
    let (Some(stated), Some(n), Some(d)) = (get(calc, derived), get(calc, num), get(calc, den))
    else {
        return;
    };
    if d.abs() < f64::EPSILON {
        return; // zero denominator: nothing to recompute
    }
    out.checked.push(rule);
    let expected = (n / d + offset) * scale;
    if !within(stated, expected, cfg) {
        out.violations.push(StructuralViolation {
            rule,
            detail: format!(
                "{derived0} = {stated}，但按 {num0}/{den0}×100 应为 {expected}（差 {delta}pp）。",
                derived0 = derived[0],
                num0 = num[0],
                den0 = den[0],
                stated = fmt(stated),
                expected = fmt(expected),
                delta = fmt((stated - expected).abs())
            ),
            expected,
            stated,
        });
    }
}

/// Run every applicable rule over the CALCULATION values.
pub fn check(calc: &BTreeMap<String, Value>, cfg: &GuardrailConfig) -> StructuralReport {
    let mut report = StructuralReport::default();

    check_ratio(
        calc,
        "margin-consistency",
        &["net_margin_pct", "net_margin"],
        &["net_profit", "net_profit_parent"],
        &["revenue"],
        100.0,
        0.0,
        cfg,
        &mut report,
    );
    check_ratio(
        calc,
        "deducted-margin",
        &["deducted_margin_pct"],
        &["net_profit_deducted"],
        &["revenue"],
        100.0,
        0.0,
        cfg,
        &mut report,
    );

    // YoY: stated growth must match the level pair, whichever alias is used.
    check_ratio(
        calc,
        "revenue-yoy",
        &["yoy_first", "revenue_yoy_pct"],
        &["revenue"],
        &["revenue_prior", "revenue_ly", "revenue_last"],
        100.0,
        -1.0,
        cfg,
        &mut report,
    );
    check_ratio(
        calc,
        "profit-yoy",
        &["net_profit_yoy_pct"],
        &["net_profit"],
        &["net_profit_prior", "net_profit_ly"],
        100.0,
        -1.0,
        cfg,
        &mut report,
    );

    // Margin range: a margin over 100% (or under −100%) is impossible for a
    // going concern — net profit cannot exceed revenue in magnitude.
    let margin_keys: Vec<String> = calc
        .keys()
        .filter(|k| k.ends_with("_margin_pct"))
        .cloned()
        .collect();
    if !margin_keys.is_empty() {
        report.checked.push("margin-range");
    }
    for k in &margin_keys {
        if let Some(v) = calc.get(k).and_then(|v| v.as_f64()) {
            if !(-100.0..=100.0).contains(&v) {
                report.checked.push("margin-range");
                report.violations.push(StructuralViolation {
                    rule: "margin-range",
                    detail: format!(
                        "{k} = {v} 超出可行域 [−100, 100]：净利润不可能超过营收。",
                        v = fmt(v)
                    ),
                    expected: v.clamp(-100.0, 100.0),
                    stated: v,
                });
            }
        }
    }

    // Balance-sheet identity: assets = liabilities + equity.
    if let (Some(a), Some(l), Some(e)) = (
        get(calc, &["assets", "total_assets"]),
        get(calc, &["liabilities", "total_liabilities"]),
        get(calc, &["equity", "owners_equity"]),
    ) {
        report.checked.push("balance-identity");
        let expected = l + e;
        if !within(a, expected, cfg) {
            report.violations.push(StructuralViolation {
                rule: "balance-identity",
                detail: format!(
                    "assets = {a}，但 liabilities + equity = {expected}，不满足资产负债表恒等式。",
                    a = fmt(a),
                    expected = fmt(expected)
                ),
                expected,
                stated: a,
            });
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn calc(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    /// The real 斯迪克 2026Q3 calculation — every rule that runs must pass.
    #[test]
    fn real_sample_passes() {
        let c = calc(&[
            ("revenue", json!(24.83)),
            ("net_profit", json!(0.669378)),
            ("net_profit_deducted", json!(0.504206)),
            ("ocf", json!(-0.74839)),
            ("yoy_first", json!(10.9)),
            ("pct_change_event", json!(47.86)),
            ("net_margin_pct", json!(2.7)),
            ("deducted_lte", json!(true)),
        ]);
        let r = check(&c, &GuardrailConfig::default());
        assert!(r.ok(), "unexpected violations: {:?}", r.violations);
        assert!(r.checked.contains(&"margin-consistency"));
        // yoy_first has no revenue_prior in the sample → rule not applicable.
        assert!(!r.checked.contains(&"revenue-yoy"));
    }

    #[test]
    fn flags_inconsistent_margin() {
        // stated 3.1 vs true 24.83×? → 0.669378/24.83×100 = 2.695 → off by 0.4pp.
        let c = calc(&[
            ("revenue", json!(24.83)),
            ("net_profit", json!(0.669378)),
            ("net_margin_pct", json!(3.1)),
        ]);
        let r = check(&c, &GuardrailConfig::default());
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].rule, "margin-consistency");
        assert!((r.violations[0].expected - 2.695).abs() < 0.01);
    }

    #[test]
    fn flags_inconsistent_yoy() {
        // 24.83 / 20.0 − 1 = 24.15% ≠ stated 10.9.
        let c = calc(&[
            ("revenue", json!(24.83)),
            ("revenue_prior", json!(20.0)),
            ("yoy_first", json!(10.9)),
        ]);
        let r = check(&c, &GuardrailConfig::default());
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].rule, "revenue-yoy");
    }

    #[test]
    fn yoy_consistent_passes() {
        let c = calc(&[
            ("revenue", json!(24.83)),
            ("revenue_prior", json!(22.39)),
            ("yoy_first", json!(10.9)),
        ]);
        let r = check(&c, &GuardrailConfig::default());
        assert!(r.ok(), "{:?}", r.violations);
        assert!(r.checked.contains(&"revenue-yoy"));
    }

    #[test]
    fn flags_impossible_margin() {
        let c = calc(&[("net_margin_pct", json!(147.5))]);
        let r = check(&c, &GuardrailConfig::default());
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].rule, "margin-range");
    }

    #[test]
    fn balance_identity() {
        let good = calc(&[
            ("assets", json!(30.0)),
            ("liabilities", json!(18.0)),
            ("equity", json!(12.0)),
        ]);
        assert!(check(&good, &GuardrailConfig::default()).ok());
        let bad = calc(&[
            ("assets", json!(30.0)),
            ("liabilities", json!(18.0)),
            ("equity", json!(10.0)),
        ]);
        let r = check(&bad, &GuardrailConfig::default());
        assert_eq!(r.violations[0].rule, "balance-identity");
    }

    #[test]
    fn empty_calc_is_clean() {
        let r = check(&BTreeMap::new(), &GuardrailConfig::default());
        assert!(r.ok());
        assert!(r.checked.is_empty());
    }
}
