//! WB-internal numeric guardrail (F4 / contract I8 semantics, implemented locally
//! per the final function list: no server endpoint).
//!
//! Rule (contract v3 §6): every numeric claim in the model-generated text must trace
//! back to evidence numbers (deterministic computed metrics + announcement excerpt
//! numbers). Default tolerance 1% relative, 0.02 absolute for percentage points.
//! Arithmetic results computed by the model (e.g. "扣非低于归母约 1652 万" = the
//! difference of two evidence numbers) are NOT evidence numbers → violation.

use regex::Regex;

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
fn number_regex() -> &'static Regex {
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

/// Run the guardrail over `text`. Byte offsets refer to `text` and are always
/// char-boundary safe (number matches start/end on ASCII digits/punctuation).
pub fn check(text: &str, evidence: &[f64], cfg: &GuardrailConfig) -> Guardrail {
    let num_re = number_regex();
    let date_re = Regex::new(r"\d{4}-\d{1,2}-\d{1,2}").unwrap();
    let url_re = Regex::new(r"https?://\S+").unwrap();
    let pct_re = Regex::new(r"（?[-+]?\d[\d,]*(?:\.\d+)?%").unwrap();

    let mut excluded: Vec<(usize, usize)> = Vec::new();
    for m in date_re.find_iter(text).chain(url_re.find_iter(text)) {
        excluded.push((m.start(), m.end()));
    }

    let mut spans = Vec::new();
    let mut violations = Vec::new();

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
        let is_pct = pct_re
            .find_iter(text)
            .any(|p| p.start() <= start && p.end() >= end);
        let raw = m.as_str();
        let Ok(value) = raw.replace(',', "").parse::<f64>() else {
            continue;
        };

        // Scale candidates: direct, 万元→亿元, 亿元→万元.
        let candidates = [value, value / 10_000.0, value * 10_000.0];
        let mut matched = None;
        for cand in candidates {
            if let Some(e) = evidence
                .iter()
                .copied()
                .find(|e| within_tolerance(cand, *e, is_pct, cfg))
            {
                matched = Some(e);
                break;
            }
        }

        let span = NumSpan {
            start,
            end,
            raw: raw.to_string(),
            value,
            ok: matched.is_some(),
            matched,
        };
        if matched.is_none() {
            violations.push(Violation {
                start,
                end,
                raw: raw.to_string(),
                value,
                hint: format!(
                    "「{raw}」未能在证据数字中回溯（容差 {tol}% / {pp}pp）。模型自算差值不是证据数字，须改引公告原始数字。",
                    tol = cfg.tolerance_pct,
                    pp = cfg.pp_tolerance
                ),
            });
        }
        spans.push(span);
    }

    Guardrail { spans, violations }
}

fn within_tolerance(candidate: f64, evidence: f64, _is_pct: bool, cfg: &GuardrailConfig) -> bool {
    let diff = (candidate - evidence).abs();
    let rel = evidence.abs() * cfg.tolerance_pct / 100.0;
    diff <= rel.max(cfg.pp_tolerance)
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
}
