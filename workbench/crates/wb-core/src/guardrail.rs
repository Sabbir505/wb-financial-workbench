//! WB-internal numeric guardrail (F4 / contract I8 semantics, implemented locally
//! per the final function list: no server endpoint).
//!
//! Rule (contract v3 §6): every numeric claim in the model-generated text must trace
//! back to evidence numbers (deterministic computed metrics + announcement excerpt
//! numbers). Default tolerance 1% relative, 0.02 absolute for percentage points.
//! Arithmetic results computed by the model (e.g. "扣非低于归母约 1652 万" = the
//! difference of two evidence numbers) are NOT evidence numbers → violation.

use regex::Regex;

use crate::binding::{self, BindingSpan, BindingVerdict, EvidenceFact, Period};

#[derive(Clone, Debug)]
pub struct GuardrailConfig {
    pub tolerance_pct: f64,
    pub pp_tolerance: f64,
}

impl Default for GuardrailConfig {
    fn default() -> Self {
        GuardrailConfig {
            tolerance_pct: 1.0,
            pp_tolerance: 0.02,
        }
    }
}

/// Sources of evidence numbers for one task.
#[derive(Clone, Debug, Default)]
pub struct EvidenceInputs {
    /// Numeric values from deterministic computed metrics (亿元 / percent points).
    pub metrics: Vec<f64>,
    /// Numbers extracted from free-text evidence (announcement summary, audit excerpt).
    pub free_texts: Vec<String>,
    /// Numbers the independent validator itself accepted (analysis explanation).
    pub checked_values: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct NumSpan {
    pub start: usize,
    pub end: usize,
    pub raw: String,
    pub value: f64,
    pub ok: bool,
    /// Evidence value that legitimized this number, for tooltips.
    pub matched: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct Violation {
    pub start: usize,
    pub end: usize,
    pub raw: String,
    pub value: f64,
    /// Human-facing hint for the tooltip.
    pub hint: String,
}

#[derive(Clone, Debug)]
pub struct Guardrail {
    pub spans: Vec<NumSpan>,
    pub violations: Vec<Violation>,
    /// One entry per numeric token, in the same order as `spans`. The value
    /// verdict is in `spans`/`violations`; this vector carries the binding
    /// verdict and is advisory (it never changes `ok()`).
    pub bindings: Vec<BindingSpan>,
}

impl Guardrail {
    pub fn ok(&self) -> bool {
        self.violations.is_empty()
    }

    /// Raw offending values, e.g. ["0.1652"] — mirrors the server-side check message.
    pub fn violation_tokens(&self) -> Vec<String> {
        self.violations
            .iter()
            .map(|v| format!("{}", trim_float(v.value)))
            .collect()
    }

    /// Numbers whose value is traceable but whose account, period, scope or
    /// unit does not match the evidence. Advisory: not part of `ok()`.
    pub fn binding_mismatches(&self) -> impl Iterator<Item = &BindingSpan> {
        self.bindings
            .iter()
            .filter(|b| matches!(b.verdict, BindingVerdict::Mismatch { .. }))
    }

    pub fn binding_mismatch_count(&self) -> usize {
        self.binding_mismatches().count()
    }

    /// `(supported, value_only, mismatch, unsupported)` over all numeric tokens.
    pub fn binding_counts(&self) -> (usize, usize, usize, usize) {
        let mut out = (0, 0, 0, 0);
        for b in &self.bindings {
            match b.verdict {
                BindingVerdict::Supported => out.0 += 1,
                BindingVerdict::ValueOnly => out.1 += 1,
                BindingVerdict::Mismatch { .. } => out.2 += 1,
                BindingVerdict::Unsupported => out.3 += 1,
            }
        }
        out
    }
}

pub fn trim_float(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// Numbers embedded in free text: ASCII digits with optional sign, thousands
/// separators and decimals. Compiled once; used by both evidence collection
/// and the check pass.
pub(crate) fn number_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[-+]?\d[\d,]*(?:\.\d+)?").unwrap())
}

/// Collect the evidence number set: metrics + every number in free texts (with
/// 万/亿 scale candidates) + previously accepted checked values.
pub fn build_evidence_numbers(inputs: &EvidenceInputs) -> Vec<f64> {
    let mut out = Vec::new();
    let mut push = |v: f64| {
        let v = trim_float(v);
        if v.is_finite() && !out.contains(&v) {
            out.push(v);
        }
    };
    for m in &inputs.metrics {
        push(*m);
    }
    for v in &inputs.checked_values {
        push(*v);
    }
    let num_re = number_regex();
    for text in &inputs.free_texts {
        for cap in num_re.find_iter(text) {
            let raw = cap.as_str().replace(',', "");
            let Ok(v) = raw.parse::<f64>() else { continue };
            push(v);
            push(v / 10_000.0); // 万元 → 亿元
            push(v * 10_000.0); // 亿元 → 万元
        }
    }
    out
}

/// One numeric token as it appears in the source text.
#[derive(Clone, Debug)]
pub(crate) struct RawNumber {
    pub start: usize,
    pub end: usize,
    pub raw: String,
    pub value: f64,
    pub is_pct: bool,
}

/// Scan `text` for number claims. This is the single place that decides what a
/// "number" is (dates, URLs and identifiers are excluded). Both the value check
/// and the binding extractor use it, so they can never disagree about which
/// tokens exist.
pub(crate) fn scan_numbers(text: &str) -> Vec<RawNumber> {
    let num_re = number_regex();
    let date_re = Regex::new(r"\d{4}-\d{1,2}-\d{1,2}").unwrap();
    let url_re = Regex::new(r"https?://\S+").unwrap();
    let pct_re = Regex::new(r"（?[-+]?\d[\d,]*(?:\.\d+)?%").unwrap();

    let mut excluded: Vec<(usize, usize)> = Vec::new();
    for m in date_re.find_iter(text).chain(url_re.find_iter(text)) {
        excluded.push((m.start(), m.end()));
    }

    let mut out = Vec::new();
    for m in num_re.find_iter(text) {
        let (start, end) = (m.start(), m.end());
        if excluded.iter().any(|(s, e)| start >= *s && end <= *e) {
            continue;
        }
        // Boundary discipline: not inside an identifier ("Q3", "2026Q3", "T+5")
        // and not a fragment of a larger token.
        let prev_ok = text[..start]
            .chars()
            .next_back()
            .map(|c| !c.is_ascii_alphanumeric())
            .unwrap_or(true);
        let next_ok = text[end..]
            .chars()
            .next()
            .map(|c| !c.is_ascii_alphabetic())
            .unwrap_or(true);
        if !prev_ok || !next_ok {
            continue;
        }
        let raw = m.as_str();
        if is_period_label(text, end, raw) {
            continue;
        }
        let is_pct = pct_re
            .find_iter(text)
            .any(|p| p.start() <= start && p.end() >= end);
        let Ok(value) = raw.replace(',', "").parse::<f64>() else {
            continue;
        };
        out.push(RawNumber {
            start,
            end,
            raw: raw.to_string(),
            value,
            is_pct,
        });
    }
    out
}

/// A bare year (`2026年`), month (`10月`) or day (`29日`) is a period label, not
/// a numeric claim. Dates such as `2026-10-29` are already covered by the date
/// exclusion. This keeps "2026年前三季度营业收入..." from flagging 2026.
fn is_period_label(text: &str, end: usize, raw: &str) -> bool {
    let digits = raw.replace(',', "");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    match text[end..].trim_start().chars().next() {
        Some('年') => digits.len() == 4,
        Some('月') | Some('日') => digits.len() <= 2,
        _ => false,
    }
}

fn violation_hint(raw: &str, cfg: &GuardrailConfig) -> String {
    format!(
        "「{raw}」未能在证据数字中回溯（容差 {tol}% / {pp}pp）。模型自算差值不是证据数字，须改引公告原始数字。",
        tol = cfg.tolerance_pct,
        pp = cfg.pp_tolerance
    )
}

/// Run the value guardrail over `text`. Byte offsets refer to `text` and are
/// always char-boundary safe (number matches start/end on ASCII digits and
/// punctuation).
///
/// Percentages are never rescaled. The old `[v, v/1e4, v*1e4]` candidates made
/// `200.71%` match `0.002483` after a 10,000x division (the 0.02pp floor then
/// accepted the result), which is a false negative. Amounts still use the
/// 万元/亿元 candidates because the brief and the metrics can be in different
/// scales.
pub fn check(text: &str, evidence: &[f64], cfg: &GuardrailConfig) -> Guardrail {
    let mut spans = Vec::new();
    let mut violations = Vec::new();

    for raw in scan_numbers(text) {
        let candidates: Vec<f64> = if raw.is_pct {
            vec![raw.value]
        } else {
            vec![raw.value, raw.value / 10_000.0, raw.value * 10_000.0]
        };
        let mut matched = None;
        for cand in candidates {
            if let Some(e) = evidence
                .iter()
                .copied()
                .find(|e| within_tolerance(cand, *e, raw.is_pct, cfg))
            {
                matched = Some(e);
                break;
            }
        }

        spans.push(NumSpan {
            start: raw.start,
            end: raw.end,
            raw: raw.raw.clone(),
            value: raw.value,
            ok: matched.is_some(),
            matched,
        });
        if matched.is_none() {
            violations.push(Violation {
                start: raw.start,
                end: raw.end,
                raw: raw.raw.clone(),
                value: raw.value,
                hint: violation_hint(&raw.raw, cfg),
            });
        }
    }

    Guardrail {
        spans,
        violations,
        bindings: Vec::new(),
    }
}

/// Run the value check and, for every value that can be traced, compare the
/// claim's metric, base metric, period, scope and unit against the matched
/// fact. `violations` stays value-only (the supervisor's acceptance metric);
/// `bindings` carries the advisory second verdict.
pub fn check_with_facts(
    text: &str,
    facts: &[EvidenceFact],
    cfg: &GuardrailConfig,
) -> Guardrail {
    let mut spans = Vec::new();
    let mut violations = Vec::new();
    let mut bindings = Vec::new();

    for raw in scan_numbers(text) {
        let cue = binding::cue_for(text, &raw, Period::unknown());
        let candidates: Vec<(usize, &EvidenceFact)> = facts
            .iter()
            .enumerate()
            .filter(|(_, f)| binding::value_matches(&cue, f, cfg))
            .collect();

        if candidates.is_empty() {
            spans.push(NumSpan {
                start: raw.start,
                end: raw.end,
                raw: raw.raw.clone(),
                value: raw.value,
                ok: false,
                matched: None,
            });
            violations.push(Violation {
                start: raw.start,
                end: raw.end,
                raw: raw.raw.clone(),
                value: raw.value,
                hint: violation_hint(&raw.raw, cfg),
            });
            bindings.push(BindingSpan {
                start: raw.start,
                end: raw.end,
                raw: raw.raw.clone(),
                value: raw.value,
                verdict: BindingVerdict::Unsupported,
                fact: None,
                hint: String::new(),
            });
            continue;
        }

        let (verdict, fact) = binding::resolve(&cue, &candidates);
        let hint = binding::hint_for(&verdict, &cue, fact.as_ref());
        let matched = fact.as_ref().map(|f| f.value);
        spans.push(NumSpan {
            start: raw.start,
            end: raw.end,
            raw: raw.raw.clone(),
            value: raw.value,
            ok: true,
            matched,
        });
        bindings.push(BindingSpan {
            start: raw.start,
            end: raw.end,
            raw: raw.raw.clone(),
            value: raw.value,
            verdict,
            fact,
            hint,
        });
    }

    Guardrail {
        spans,
        violations,
        bindings,
    }
}

/// Two tolerance regimes (contract v3 §6 / supervisor spec §2.1):
///
/// - **percentages** (`47.86` = 47.86%): 1% relative, floored at `pp_tolerance`
///   (0.02 percentage points) — the floor is what lets a rounded 2.7% match a
///   reported 2.68%.
/// - **amounts** (亿元): 1% relative only. The 0.02 absolute floor is NOT applied
///   here: at 亿 scale it would legitimise any figure within 0.02 of an evidence
///   value (2% of a 1.0亿 metric), which is wider than the relative band the
///   contract specifies. Exact zeros still match exactly.
pub(crate) fn within_tolerance(candidate: f64, evidence: f64, is_pct: bool, cfg: &GuardrailConfig) -> bool {
    let diff = (candidate - evidence).abs();
    let rel = evidence.abs() * cfg.tolerance_pct / 100.0;
    if is_pct {
        diff <= rel.max(cfg.pp_tolerance)
    } else {
        diff <= rel.max(f64::EPSILON * evidence.abs().max(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_evidence() -> Vec<f64> {
        build_evidence_numbers(&EvidenceInputs {
            metrics: vec![
                24.83, 0.669378, 0.504206, -0.74839, 10.9, 47.86, 2.7,
            ],
            free_texts: vec![
                "斯迪克发布2026年三季度报告：前三季度营业收入24.83亿元，同比增长10.90%；归母净利润6693.78万元，同比增长47.86%；扣非净利润5042.06万元，同比增长98.64%。第三季度单季营收8.90亿元，同比增长5.57%，归母净利润2092.60万元，同比增长4.41%。经营活动现金流量净额-7483.90万元，同比下降127.02%，主要系销售规模增加导致存货和应收账款营运资金投入增加。在建工程同比增长200.71%，主要系斯迪克高端功能性膜材扩产项目、共跃星辰磁性材料项目陆续进入建设期。".to_string(),
            ],
            checked_values: vec![10.9, 47.86, 98.64, -0.74839, -127.02, 2.7],
        })
    }

    /// The real blocked case: "扣非低于归母约 1652 万" is a model-computed
    /// difference (0.669378 − 0.504206 ≈ 0.1652亿) and must be flagged.
    #[test]
    fn flags_model_invented_difference_1652wan() {
        let text = "经营变化：前三季营收+10.90%、归母净利+47.86%、扣非+98.64%；Q3单季增速明显回落；在建工程+200.71%，扩产项目进入建设期。\n风险点：经营现金流-7483.90万元（-127.02%），存货及应收账款占用增加；净利率约2.7%，盈利偏薄；扣非低于归母约1652万元。\n待核实问题：膜材扩产与磁性材料项目投产节奏及产能消化；非经常性损益具体构成；四季度经营现金流能否改善。";
        let g = check(text, &sample_evidence(), &GuardrailConfig::default());
        assert!(!g.ok(), "expected the 1652万 violation");
        assert_eq!(g.violations.len(), 1, "{:?}", g.violations);
        let v = &g.violations[0];
        assert_eq!(v.raw, "1652");
        assert!((v.value - 1652.0).abs() < 1e-9);
        assert_eq!(g.violation_tokens(), vec!["1652"]);
    }

    /// The corrected rewrite quotes the announcement's own numbers → passes.
    #[test]
    fn corrected_rewrite_passes() {
        let text = "经营变化：前三季营收19.31亿元（+10.29%），归母净利润0.31亿元（-61.27%），净利率1.62%。\n风险点：盈利大幅回落，扣非净利润0.07亿元，盈利质量承压。\n待核实问题：四季度订单与价格走势；产能利用率恢复情况。";
        let evidence = build_evidence_numbers(&EvidenceInputs {
            metrics: vec![19.31, 0.31, 0.07, 1.62, 10.29, -61.27],
            free_texts: vec![],
            checked_values: vec![],
        });
        let g = check(text, &evidence, &GuardrailConfig::default());
        assert!(g.ok(), "unexpected violations: {:?}", g.violations);
    }

    #[test]
    fn ignores_identifiers_dates_and_urls() {
        let text = "2026-10-29 披露，见 http://static.cninfo.com.cn/finalpage/2026-10-29/1225636931.PDF 与 2026Q3 报告、T+5 评价";
        let g = check(text, &sample_evidence(), &GuardrailConfig::default());
        assert!(g.ok(), "violations: {:?}", g.violations);
    }

    #[test]
    fn tolerance_window() {
        let cfg = GuardrailConfig::default();
        assert!(within_tolerance(24.85, 24.83, false, &cfg)); // 0.08% off
        assert!(within_tolerance(24.9, 24.83, false, &cfg)); // 0.28% off, inside the 1% band
        assert!(!within_tolerance(25.2, 24.83, false, &cfg)); // 1.5% off
        assert!(within_tolerance(1652.0, 1651.72, false, &cfg)); // would pass IF it were evidence
        assert!(within_tolerance(2.71, 2.7, true, &cfg)); // 0.01pp inside the pp floor
        assert!(!within_tolerance(2.75, 2.7, true, &cfg)); // 0.05pp outside
        assert!(!within_tolerance(-129.0, -127.02, true, &cfg)); // 1.56% off
    }

    /// The 0.02 absolute floor belongs to percentages only (contract v3 §6,
    /// supervisor spec §2.1 "0.02pp for percentages"). On an amount it would
    /// accept a figure 2% away from a 1.0亿 metric — wider than the 1% band.
    #[test]
    fn pp_floor_does_not_apply_to_amounts() {
        let cfg = GuardrailConfig::default();
        // Same numbers, different regimes.
        assert!(within_tolerance(1.015, 1.0, true, &cfg)); // 0.015pp, floor saves it
        assert!(!within_tolerance(1.015, 1.0, false, &cfg)); // 1.5% off an amount
        // The relative band still governs amounts.
        assert!(within_tolerance(1.005, 1.0, false, &cfg)); // 0.5% off
        // An amount far below the floor is not legitimised by proximity to zero.
        assert!(!within_tolerance(0.015, 0.0, false, &cfg));
        assert!(within_tolerance(0.0, 0.0, false, &cfg)); // exact zero still matches
    }

    #[test]
    fn period_labels_are_not_number_claims() {
        let g = check(
            "2026年前三季度营业收入24.83亿元",
            &[24.83],
            &GuardrailConfig::default(),
        );
        assert!(g.ok(), "violations: {:?}", g.violations);
    }

    /// Regression: `200.71%` used to pass because the percentage was divided by
    /// 10,000 and the 0.02pp floor accepted the result against `0.002483`
    /// (a scale variant of revenue). Percentages are never rescaled now.
    #[test]
    fn percentage_is_not_rescaled_into_an_amount() {
        let g = check(
            "在建工程增长200.71%",
            &[0.002483],
            &GuardrailConfig::default(),
        );
        assert_eq!(g.violation_tokens(), vec!["200.71"]);
    }

    fn seed_inference() -> &'static str {
        "经营变化：前三季营收+10.90%、归母净利+47.86%、扣非+98.64%；Q3单季增速明显回落；在建工程+200.71%，扩产项目进入建设期。\n风险点：经营现金流-7483.90万元（-127.02%），存货及应收账款占用增加；净利率约2.7%，盈利偏薄；扣非低于归母约1652万元。\n待核实问题：膜材扩产与磁性材料项目投产节奏及产能消化；非经常性损益具体构成；四季度经营现金流能否改善。"
    }

    fn seed_facts() -> Vec<EvidenceFact> {
        let summary = "摘要：前三季度营业收入24.83亿元，同比增长10.90%；归母净利润6693.78万元，同比增长47.86%；扣非净利润5042.06万元，同比增长98.64%。第三季度单季营收8.90亿元，同比增长5.57%，归母净利润2092.60万元，同比增长4.41%。经营活动现金流量净额-7483.90万元，同比下降127.02%。";
        let excerpt = "营业收入 2,482,691,849.55 元（同比 +10.90%）；归属于上市公司股东的净利润 66,937,816.48 元（同比 +47.86%）；归属于上市公司股东的扣除非经常性损益的净利润 50,420,578.11 元（同比 +98.64%）；经营活动产生的现金流量净额 -74,838,960.79 元（同比 -127.02%）。";
        let mut metrics = std::collections::BTreeMap::new();
        for (k, v) in [
            ("revenue", 24.83),
            ("net_profit", 0.669378),
            ("net_profit_deducted", 0.504206),
            ("ocf", -0.74839),
            ("yoy_first", 10.9),
            ("pct_change_event", 47.86),
            ("net_margin_pct", 2.7),
        ] {
            metrics.insert(k.to_string(), serde_json::Value::from(v));
        }
        let mut facts = Vec::new();
        facts.extend(crate::binding::facts_from_text(
            summary,
            crate::binding::Period::unknown(),
            crate::binding::FactSource::Excerpt,
            "summary",
        ));
        facts.extend(crate::binding::facts_from_text(
            excerpt,
            crate::binding::Period::unknown(),
            crate::binding::FactSource::Excerpt,
            "audit",
        ));
        facts.extend(crate::binding::facts_from_metrics(
            &metrics,
            "亿元",
            crate::binding::Period::unknown(),
            crate::binding::FactSource::Calc,
        ));
        facts
    }

    /// The seed case, with the real excerpt and summary. The two untraceable
    /// figures are the model's own difference (1652) and the construction
    /// growth rate (200.71) that the old scale-candidate path let through.
    /// Everything else must bind: no amber flags on the clean figures.
    #[test]
    fn seed_case_value_and_binding_are_separated() {
        let facts = seed_facts();
        let g = check_with_facts(seed_inference(), &facts, &GuardrailConfig::default());
        assert_eq!(g.violation_tokens(), vec!["200.71", "1652"]);
        assert_eq!(g.binding_mismatch_count(), 0, "{:?}", g.bindings);
        assert_eq!(g.binding_counts(), (6, 0, 0, 2));
    }

    /// A binding mismatch is advisory: it must not change the value verdict.
    #[test]
    fn binding_mismatch_does_not_change_ok() {
        let fact = EvidenceFact {
            metric_id: Some("revenue".into()),
            base_metric_id: None,
            value: 2_483_000_000.0,
            native: 24.83,
            unit: crate::binding::Unit::Amount,
            scale: crate::binding::AmountScale::Yi,
            period: crate::binding::Period::unknown(),
            scope: crate::binding::Scope::Unknown,
            source: crate::binding::FactSource::Metric,
            label: "营业收入".into(),
            locator: "test".into(),
        };
        let g = check_with_facts(
            "归母净利润24.83亿元",
            &[fact],
            &GuardrailConfig::default(),
        );
        assert!(g.ok(), "value is traceable: {:?}", g.violations);
        assert_eq!(g.binding_mismatch_count(), 1);
        assert!(matches!(
            g.bindings[0].verdict,
            BindingVerdict::Mismatch {
                dimension: crate::binding::BindingDimension::Metric,
                ..
            }
        ));
    }

    #[test]
    fn binding_flags_wrong_period_without_a_value_flag() {
        let fact = EvidenceFact {
            metric_id: Some("revenue".into()),
            base_metric_id: None,
            value: 2_483_000_000.0,
            native: 24.83,
            unit: crate::binding::Unit::Amount,
            scale: crate::binding::AmountScale::Yi,
            period: crate::binding::Period {
                year: Some(2026),
                span: crate::binding::PeriodSpan::Q2,
            },
            scope: crate::binding::Scope::Unknown,
            source: crate::binding::FactSource::Metric,
            label: "营业收入".into(),
            locator: "test".into(),
        };
        let g = check_with_facts(
            "2026Q3营业收入24.83亿元",
            &[fact],
            &GuardrailConfig::default(),
        );
        assert!(g.ok());
        assert_eq!(g.binding_mismatch_count(), 1);
        assert!(matches!(
            g.bindings[0].verdict,
            BindingVerdict::Mismatch {
                dimension: crate::binding::BindingDimension::Period,
                ..
            }
        ));
    }
}
