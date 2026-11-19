//! Evidence matching (F4 layer 3, advisory): link every number in the
//! model-written narrative to the *source sentence* that supports it.
//!
//! The guardrail answers "is this number traceable?" — a boolean over the whole
//! evidence set. This module answers the reviewer's follow-up question:
//! "traceable to what?" Every number gets the field it came from (a
//! deterministic metric, a CALCULATION value, or a sentence in the announcement
//! excerpt) plus the sentence itself.
//!
//! Advisory only. Nothing here approves, rejects or blocks a brief, and no
//! number becomes valid because it carries a citation — validity is still
//! decided by `guardrail::check`. This module explains a verdict; it does not
//! produce one.
//!
//! ## Why the span set is not recomputed
//!
//! `cite` calls `guardrail::check_with_facts` and resolves each span it
//! returns, so the citation set is exactly the guardrail's span set by
//! construction: a number can never be flagged by one and cited by the other.
//! The fact set is built from the same labelled evidence, so the value and
//! binding verdicts and the citations cannot drift apart.

use crate::binding::{facts_from_metrics, facts_from_text, EvidenceFact, FactSource, Period};
use crate::guardrail::{check_with_facts, trim_float, GuardrailConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Deterministic computed metric from the context bundle.
    Metric,
    /// Value from the brief's own CALCULATION section (tool output).
    Calc,
    /// Number inside the announcement / evidence excerpt.
    Excerpt,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Metric => "metric",
            SourceKind::Calc => "calc",
            SourceKind::Excerpt => "excerpt",
        }
    }
}

/// Where a brief number came from.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceRef {
    pub kind: SourceKind,
    /// Metric key, CALCULATION key, or the excerpt's label.
    pub key: String,
    /// The evidence value that legitimised the number (already scale-normalised
    /// by the guardrail's candidate search).
    pub evidence: f64,
    /// The sentence in the source that contains the number. Empty for
    /// metrics/CALC values, which have no surrounding prose.
    pub quote: String,
    /// Byte offsets of `quote` inside the source text (excerpts only).
    pub quote_start: usize,
    pub quote_end: usize,
}

/// One number in the model-written text, with its provenance if it has any.
#[derive(Clone, Debug, PartialEq)]
pub struct Citation {
    pub start: usize,
    pub end: usize,
    pub raw: String,
    pub value: f64,
    /// `None` means untraceable — the same condition the guardrail reports as a
    /// violation.
    pub source: Option<SourceRef>,
}

impl Citation {
    pub fn traced(&self) -> bool {
        self.source.is_some()
    }
}

/// Evidence with labels, so a match can be attributed to its origin.
#[derive(Clone, Debug, Default)]
pub struct LabelledEvidence {
    /// Deterministic metrics: `(key, value)`.
    pub metrics: Vec<(String, f64)>,
    /// The brief's own CALCULATION values: `(key, value)`.
    pub calc: Vec<(String, f64)>,
    /// Free-text evidence: `(label, text)` — announcement summary, audit excerpt.
    pub excerpts: Vec<(String, String)>,
    /// Structured evidence-pack facts. These carry the unit, period and scope
    /// that the prose sources cannot, so they must participate in both the
    /// guardrail and the citation set.
    pub dto: Vec<EvidenceFact>,
}

impl LabelledEvidence {
    /// The flat evidence array, in the same order as
    /// `guardrail::build_evidence_numbers`.
    pub fn values(&self) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::new();
        for (v, _) in evidence_sources(self) {
            if !out.contains(&v) {
                out.push(v);
            }
        }
        out
    }
}

/// A number as it appears in a source, with enough context to quote it.
#[derive(Clone, Debug)]
struct SourceDraft {
    kind: SourceKind,
    key: String,
    /// The excerpt this number came from, and where it sits in it.
    text: Option<String>,
    offset: Option<(usize, usize)>,
}

/// Mirror of `guardrail::build_evidence_numbers` that keeps the origin of each
/// value. Order and de-duplication must match exactly — see the
/// `mirrors_guardrail_evidence` test.
/// One entry per *origin*, not per value: `24.83` arrives from both the metric
/// and the announcement excerpt, and the duplicate is what lets a citation quote
/// the sentence. `values()` de-duplicates; matching does not need to.
fn evidence_sources(ev: &LabelledEvidence) -> Vec<(f64, SourceDraft)> {
    let mut out: Vec<(f64, SourceDraft)> = Vec::new();
    let mut push = |v: f64, draft: SourceDraft| {
        let v = trim_float(v);
        if v.is_finite() {
            out.push((v, draft));
        }
    };

    for (key, v) in &ev.metrics {
        push(
            *v,
            SourceDraft {
                kind: SourceKind::Metric,
                key: key.clone(),
                text: None,
                offset: None,
            },
        );
    }
    for (key, v) in &ev.calc {
        push(
            *v,
            SourceDraft {
                kind: SourceKind::Calc,
                key: key.clone(),
                text: None,
                offset: None,
            },
        );
    }
    for fact in &ev.dto {
        push(
            fact.native,
            SourceDraft {
                kind: match fact.source {
                    FactSource::Calc => SourceKind::Calc,
                    FactSource::Metric => SourceKind::Metric,
                    FactSource::Excerpt | FactSource::Legacy => SourceKind::Excerpt,
                },
                key: fact
                    .metric_id
                    .clone()
                    .unwrap_or_else(|| fact.label.clone()),
                text: None,
                offset: None,
            },
        );
    }
    for (label, text) in &ev.excerpts {
        for m in crate::guardrail::number_regex().find_iter(text) {
            let raw = m.as_str().replace(',', "");
            let Ok(v) = raw.parse::<f64>() else { continue };
            let draft = || SourceDraft {
                kind: SourceKind::Excerpt,
                key: label.clone(),
                text: Some(text.clone()),
                offset: Some((m.start(), m.end())),
            };
            // Same three scale candidates as the guardrail: direct, 万元→亿元,
            // 亿元→万元. The draft keeps the *original* offset so the quote is
            // the sentence where the number is written, not a scaled variant.
            push(v, draft());
            push(v / 10_000.0, draft());
            push(v * 10_000.0, draft());
        }
    }
    out
}

/// The sentence containing `[start, end)` in `text`, trimmed of surrounding
/// whitespace, with its byte offsets. Sentence boundaries are CJK and ASCII
/// terminators plus newlines; a clause never crosses a blank line.
/// `start..end` is the number's own span, so a decimal point inside `24.83`
/// never counts as a sentence terminator.
fn sentence_at(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut s = 0;
    for (i, ch) in text.char_indices() {
        if i >= start {
            break;
        }
        if is_terminator(ch) {
            s = i + ch.len_utf8();
        }
    }
    let mut e = text.len();
    for (i, ch) in text.char_indices() {
        if i < end {
            continue;
        }
        if is_terminator(ch) {
            e = i + ch.len_utf8();
            break;
        }
    }
    trim_bounds(text, s, e)
}

/// Sentence boundaries. A citation should quote the clause the number sits in,
/// so commas count as terminators: `风险点：经营现金流-7483.90万元（-127.02%）`
/// must not drag the following clause in with it.
fn is_terminator(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '.' | '!' | '?' | ';' | ',' | ':' | '。' | '；' | '，' | '：' | '！' | '？'
    )
}

fn trim_bounds(text: &str, mut s: usize, mut e: usize) -> (usize, usize) {
    while s < e && !text.is_char_boundary(s) {
        s += 1;
    }
    while e > s && !text.is_char_boundary(e) {
        e -= 1;
    }
    let seg = &text[s..e];
    let lead = seg.len() - seg.trim_start().len();
    let tail = seg.len() - seg.trim_end().len();
    (s + lead, e - tail)
}

/// Build the fact set that mirrors `LabelledEvidence`. The unit is the fixtures'
/// 亿元 convention; percentages are detected from the key name.
fn labelled_facts(ev: &LabelledEvidence) -> Vec<EvidenceFact> {
    let mut facts = Vec::new();
    for (key, value) in &ev.metrics {
        let mut one = std::collections::BTreeMap::new();
        one.insert(key.clone(), serde_json::Value::from(*value));
        facts.extend(facts_from_metrics(
            &one,
            "亿元",
            Period::unknown(),
            FactSource::Metric,
        ));
    }
    for (key, value) in &ev.calc {
        let mut one = std::collections::BTreeMap::new();
        one.insert(key.clone(), serde_json::Value::from(*value));
        facts.extend(facts_from_metrics(
            &one,
            "亿元",
            Period::unknown(),
            FactSource::Calc,
        ));
    }
    for (label, text) in &ev.excerpts {
        facts.extend(facts_from_text(
            text,
            Period::unknown(),
            FactSource::Excerpt,
            label,
        ));
    }
    facts.extend(ev.dto.clone());
    facts
}

fn same_number(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Resolve every number in `text` to its source, using exactly the span set the
/// guardrail reports. The guardrail is the unit-aware `check_with_facts`, so a
/// unit-mismatched number is flagged and therefore has no citation.
pub fn cite(text: &str, ev: &LabelledEvidence, cfg: &GuardrailConfig) -> Vec<Citation> {
    let sources = evidence_sources(ev);
    let facts = labelled_facts(ev);
    let g = check_with_facts(text, &facts, cfg);

    g.spans
        .iter()
        .zip(g.bindings.iter())
        .map(|(span, binding)| {
            let source = if span.ok {
                binding.fact.as_ref().and_then(|fact| {
                    // Prefer a source we can quote. A number present in both a
                    // deterministic metric and the announcement excerpt resolves
                    // to the excerpt, because "营业收入24.83亿元" is what a
                    // reviewer can check, while "revenue" only restates the
                    // number. Compare on the fact's native value: the sources
                    // keep the value as written, not the normalised yuan value.
                    let candidates =
                        || sources.iter().filter(|(v, _)| same_number(*v, fact.native));
                    let (value, draft) = candidates()
                        .find(|(_, d)| d.text.is_some())
                        .or_else(|| candidates().next())?;
                    let (quote, quote_start, quote_end) = match (&draft.text, draft.offset) {
                        (Some(t), Some((ns, ne))) => {
                            let (s, e) = sentence_at(t, ns, ne);
                            (t[s..e].to_string(), s, e)
                        }
                        _ => (String::new(), 0, 0),
                    };
                    Some(SourceRef {
                        kind: draft.kind,
                        key: draft.key.clone(),
                        evidence: *value,
                        quote,
                        quote_start,
                        quote_end,
                    })
                })
            } else {
                None
            };
            Citation {
                start: span.start,
                end: span.end,
                raw: span.raw.clone(),
                value: span.value,
                source,
            }
        })
        .collect()
}

/// Untraceable numbers only — the reviewer-facing "these need a source" list.
pub fn untraced(citations: &[Citation]) -> Vec<&Citation> {
    citations.iter().filter(|c| !c.traced()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> GuardrailConfig {
        GuardrailConfig::default()
    }

    /// The 斯迪克 seed case: the model subtracted two real figures itself.
    fn sdic() -> LabelledEvidence {
        LabelledEvidence {
            metrics: vec![
                ("net_profit".into(), 0.669378),
                ("net_profit_deducted".into(), 0.504206),
                ("revenue".into(), 24.83),
                ("yoy_first".into(), 10.9),
            ],
            calc: vec![("net_margin_pct".into(), 2.7)],
            excerpts: vec![(
                "公告摘要".into(),
                "斯迪克发布2026年三季度报告：前三季度营业收入24.83亿元，同比增长10.90%；\
                 归母净利润6693.78万元，同比增长47.86%；扣非净利润5042.06万元，同比增长98.64%。"
                    .into(),
            )],
            dto: Vec::new(),
        }
    }

    #[test]
    fn mirrors_guardrail_evidence() {
        use crate::guardrail::{build_evidence_numbers, EvidenceInputs};
        let ev = sdic();
        let mut inputs = EvidenceInputs::default();
        inputs.metrics = ev.metrics.iter().map(|(_, v)| *v).collect();
        inputs.checked_values = ev.calc.iter().map(|(_, v)| *v).collect();
        inputs
            .free_texts
            .extend(ev.excerpts.iter().map(|(_, t)| t.clone()));
        // `build_evidence_numbers` also folds in `checked_values`; this evidence
        // set has none, so the two must agree exactly — but as sets, because the
        // guardrail's dedup order depends on iteration, not on meaning.
        let mut mine = ev.values();
        let mut theirs = build_evidence_numbers(&inputs);
        mine.sort_by(|a, b| a.partial_cmp(b).unwrap());
        theirs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            mine, theirs,
            "provenance must see exactly the numbers the guardrail sees"
        );
    }

    #[test]
    fn invented_subtraction_has_no_source() {
        let text = "扣非净利低于归母净利约1652万元。";
        let cites = cite(text, &sdic(), &cfg());
        let c = cites.iter().find(|c| c.raw == "1652").expect("1652 span");
        assert!(!c.traced(), "1652 was produced by the model, not the sources");
        assert_eq!(untraced(&cites).len(), 1);
    }

    #[test]
    fn reported_number_cites_its_sentence() {
        let text = "前三季度营业收入24.83亿元。";
        let cites = cite(text, &sdic(), &cfg());
        let c = cites.iter().find(|c| c.raw == "24.83").expect("span");
        let src = c.source.as_ref().expect("24.83 is in the metrics");
        assert!(src.quote.contains("营业收入24.83亿元"), "got {:?}", src.quote);
        assert!(src.quote_start < src.quote_end);
    }

    /// A fact that exists only in the structured pack (no prose source) must
    /// still be cited. Otherwise the guardrail could pass a number that the
    /// citation layer marks untraced, which is exactly the invariant this
    /// module exists to protect.
    #[test]
    fn dto_only_fact_is_cited() {
        use crate::binding::{AmountScale, Scope, Unit};
        let ev = LabelledEvidence {
            dto: vec![EvidenceFact {
                metric_id: Some("revenue".into()),
                base_metric_id: None,
                value: 2_483_000_000.0,
                native: 24.83,
                unit: Unit::Amount,
                scale: AmountScale::Yi,
                period: Period::unknown(),
                scope: Scope::Unknown,
                source: FactSource::Excerpt,
                label: "营业收入".into(),
                locator: "PDF 第 2 页".into(),
            }],
            ..Default::default()
        };
        let cites = cite("营收24.83亿元。", &ev, &cfg());
        let c = cites.iter().find(|c| c.raw == "24.83").expect("span");
        let src = c.source.as_ref().expect("DTO fact must be cited");
        assert_eq!(src.kind, SourceKind::Excerpt);
        assert_eq!(src.key, "revenue");
        assert!((src.evidence - 24.83).abs() < 1e-9);
    }

    #[test]
    fn citation_set_equals_guardrail_violation_set() {
        // The invariant the module is built on: a number can never be cited by
        // one and flagged by the other.
        let text = "营收24.83亿元，净利率约2.7%，扣非低于归母约1652万元。";
        let ev = sdic();
        let cites = cite(text, &ev, &cfg());
        let g = check_with_facts(text, &labelled_facts(&ev), &cfg());
        let mut flagged: Vec<&str> = g.violations.iter().map(|v| v.raw.as_str()).collect();
        let mut uncited: Vec<&str> = untraced(&cites).iter().map(|c| c.raw.as_str()).collect();
        flagged.sort();
        uncited.sort();
        assert_eq!(flagged, uncited);
    }

    #[test]
    fn quote_is_the_source_sentence_not_the_brief_sentence() {
        // The quote comes from the *evidence*, not from the text being cited.
        let ev = LabelledEvidence {
            excerpts: vec![(
                "公告摘要".into(),
                "第一句话。第二句话含24.83亿元。第三句话。".into(),
            )],
            ..Default::default()
        };
        let cites = cite("营收24.83亿元。", &ev, &cfg());
        let c = cites.iter().find(|c| c.raw == "24.83").expect("span");
        let q = &c.source.as_ref().unwrap().quote;
        assert_eq!(q, "第二句话含24.83亿元。");
    }

    #[test]
    fn decimal_point_is_not_a_sentence_boundary() {
        // `24.83` contains '.', which must not cut the quote in half.
        let ev = LabelledEvidence {
            excerpts: vec![("公告摘要".into(), "营业收入24.83亿元。下一句。".into())],
            ..Default::default()
        };
        let cites = cite("营收24.83亿元", &ev, &cfg());
        let c = cites.iter().find(|c| c.raw == "24.83").expect("span");
        assert_eq!(c.source.as_ref().unwrap().quote, "营业收入24.83亿元。");
    }

    #[test]
    fn clause_terminator_keeps_quote_tight() {
        // A comma ends the clause: the risk sentence must not drag the next one in.
        let ev = LabelledEvidence {
            excerpts: vec![(
                "公告摘要".into(),
                "风险点：经营现金流-7483.90万元，存货占用增加。".into(),
            )],
            ..Default::default()
        };
        let cites = cite("现金流-7483.90万元", &ev, &cfg());
        // The regex captures the sign, so the span's raw text is "-7483.90".
        let c = cites.iter().find(|c| c.raw == "-7483.90").expect("span");
        let q = &c.source.as_ref().unwrap().quote;
        assert!(q.contains("7483.90"), "got {q:?}");
        assert!(!q.contains("存货"), "quote crossed a clause boundary: {q:?}");
    }

    #[test]
    fn empty_text_yields_no_citations() {
        assert!(cite("", &sdic(), &cfg()).is_empty());
    }
}
