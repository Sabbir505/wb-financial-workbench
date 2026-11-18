//! Review analytics (F13): turn the local-first audit trail into quality
//! metrics and a model-error taxonomy.
//!
//! Every 修改后通过 (approve-as-revised) record carries the original draft, the
//! submitted revision and the guardrail tokens the reviewer saw. Classifying
//! the diff between the two drafts yields a deterministic, auditable taxonomy:
//!
//! - StaleData          — period/report tokens updated (2026Q3 → 2026Q4 …)
//! - HallucinatedNumber — a deleted number matches a guardrail violation
//! - Arithmetic         — a numeric edit where the old number *was* valid
//!                        evidence (the model recomputed something)
//! - Wording            — only non-numeric text changed
//! - Other              — anything else
//!
//! All classification is lexical/deterministic on the stored texts — no model
//! in the loop, so the analytics are reproducible at defense time.

use crate::audit::AuditRecord;
use crate::diff::{diff_lines, DiffOp};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorCategory {
    StaleData,
    HallucinatedNumber,
    Arithmetic,
    Wording,
    Other,
}

impl ErrorCategory {
    pub fn id(&self) -> &'static str {
        match self {
            ErrorCategory::StaleData => "stale-data",
            ErrorCategory::HallucinatedNumber => "hallucinated-number",
            ErrorCategory::Arithmetic => "arithmetic",
            ErrorCategory::Wording => "wording",
            ErrorCategory::Other => "other",
        }
    }

    pub const ALL: [ErrorCategory; 5] = [
        ErrorCategory::StaleData,
        ErrorCategory::HallucinatedNumber,
        ErrorCategory::Arithmetic,
        ErrorCategory::Wording,
        ErrorCategory::Other,
    ];
}

#[derive(Clone, Debug, Default)]
pub struct CompanyStat {
    pub company: String,
    pub reviews: usize,
    pub approvals: usize,
    pub revisions: usize,
    pub rejects: usize,
}

#[derive(Clone, Debug, Default)]
pub struct AnalyticsSummary {
    pub total_reviews: usize,
    pub approves: usize,
    pub revisions: usize,
    pub rejects: usize,
    /// approvals / total (0.0 when no reviews yet).
    pub first_pass_rate: f64,
    /// revisions / total.
    pub revision_rate: f64,
    /// Reviews that reached the reviewer with guardrail flags attached.
    pub guardrail_flagged: usize,
    /// Of those, how many ended in 修改后通过 — the reviewer confirmed the flag.
    pub guardrail_confirmed: usize,
    /// The offending tokens that recurred most, most frequent first.
    pub top_tokens: Vec<(String, usize)>,
    pub taxonomy: Vec<(ErrorCategory, usize)>,
    pub per_company: Vec<CompanyStat>,
}

/// Period/plate tokens whose edit means the draft was written against stale data.
fn is_period_token_edit(old: &str, new: &str) -> bool {
    let period = |s: &str| {
        let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        !digits.is_empty() && (s.contains('Q') || s.contains('H') || s.contains('年'))
    };
    period(old) && period(new) && old != new
}

/// True when the two strings differ only in their digits (same skeleton) —
/// a recomputed / corrected number.
fn differs_only_in_digits(old: &str, new: &str) -> bool {
    let skeleton = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .collect::<String>()
    };
    skeleton(old) == skeleton(new) && old != new
}

/// Classify one revision. `guardrail_tokens` are the raw values (e.g. "1652")
/// the guardrail flagged on the original draft.
pub fn classify_revision(
    original: &str,
    revised: &str,
    guardrail_tokens: &[String],
) -> ErrorCategory {
    let diff = diff_lines(original, revised);
    let removed: Vec<&str> = diff
        .iter()
        .filter(|d| d.op == DiffOp::Delete)
        .map(|d| d.text.as_str())
        .collect();
    let added: Vec<&str> = diff
        .iter()
        .filter(|d| d.op == DiffOp::Insert)
        .map(|d| d.text.as_str())
        .collect();
    if removed.is_empty() && added.is_empty() {
        return ErrorCategory::Other;
    }

    // Pair up consecutive remove/insert runs for token-level comparisons.
    for del in &removed {
        for ins in &added {
            if is_period_token_edit(del, ins) {
                return ErrorCategory::StaleData;
            }
        }
    }
    for del in &removed {
        let has_digit = del.chars().any(|c| c.is_ascii_digit());
        if !has_digit {
            continue;
        }
        let hit = guardrail_tokens
            .iter()
            .any(|t| !t.is_empty() && del.contains(t.as_str()));
        if hit {
            return ErrorCategory::HallucinatedNumber;
        }
    }
    // Numeric edits that are not hallucinations: matched remove/insert pairs
    // differing only in digits, or any numeric rewrite at all.
    for del in &removed {
        for ins in &added {
            if differs_only_in_digits(del, ins) {
                return ErrorCategory::Arithmetic;
            }
        }
    }
    if removed.iter().any(|d| d.chars().any(|c| c.is_ascii_digit()))
        || added.iter().any(|a| a.chars().any(|c| c.is_ascii_digit()))
    {
        return ErrorCategory::Arithmetic;
    }
    ErrorCategory::Wording
}

/// Summarize the whole audit trail.
pub fn analyze(records: &[AuditRecord]) -> AnalyticsSummary {
    let mut s = AnalyticsSummary::default();
    let mut taxonomy = BTreeMap::new();
    let mut companies: BTreeMap<String, CompanyStat> = BTreeMap::new();
    let mut tokens: BTreeMap<String, usize> = BTreeMap::new();

    for r in records {
        s.total_reviews += 1;
        match r.verdict.as_str() {
            "通过" => s.approves += 1,
            "修改后通过" => s.revisions += 1,
            "驳回" => s.rejects += 1,
            _ => {}
        }
        if !r.guardrail_tokens.is_empty() {
            s.guardrail_flagged += 1;
            if r.verdict == "修改后通过" {
                s.guardrail_confirmed += 1;
            }
            for t in &r.guardrail_tokens {
                *tokens.entry(t.clone()).or_insert(0) += 1;
            }
        }
        let cs = companies.entry(r.object_name.clone()).or_default();
        cs.company = r.object_name.clone();
        cs.reviews += 1;
        match r.verdict.as_str() {
            "通过" => cs.approvals += 1,
            "修改后通过" => cs.revisions += 1,
            "驳回" => cs.rejects += 1,
            _ => {}
        }
        if r.verdict == "修改后通过" {
            if let (Some(orig), Some(rev)) = (&r.original_inference, &r.revised_inference) {
                let cat = classify_revision(orig, rev, &r.guardrail_tokens);
                *taxonomy.entry(cat).or_insert(0) += 1;
            }
        }
    }

    if s.total_reviews > 0 {
        s.first_pass_rate = s.approves as f64 / s.total_reviews as f64;
        s.revision_rate = s.revisions as f64 / s.total_reviews as f64;
    }
    s.taxonomy = ErrorCategory::ALL
        .into_iter()
        .map(|c| (c, taxonomy.get(&c).copied().unwrap_or(0)))
        .collect();
    s.top_tokens = {
        let mut v: Vec<(String, usize)> = tokens.into_iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        v
    };
    s.per_company = companies.into_values().collect();
    s.per_company.sort_by_key(|c| std::cmp::Reverse(c.reviews));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditRecord;
    use crate::model::ReviewReceipt;

    fn receipt() -> ReviewReceipt {
        ReviewReceipt {
            ok: true,
            review_id: "RV-1".into(),
            status: "SUCCEEDED".into(),
            brief_version: Some(2),
            trace_id: None,
            reviewed_at: None,
            note: None,
        }
    }

    fn rec(verdict: &str, tokens: &[&str], orig: Option<&str>, rev: Option<&str>) -> AuditRecord {
        let req = crate::model::ReviewRequest {
            task_id: "T1".into(),
            verdict: match verdict {
                "通过" => crate::model::Verdict::Approve,
                "驳回" => crate::model::Verdict::Reject,
                _ => crate::model::Verdict::ApproveAsRevised,
            },
            comment: None,
            reviewer: "张研".into(),
            idempotency_key: "k".into(),
            revised_sections: None,
        };
        AuditRecord {
            ts: "2026-11-02T09:30:00".into(),
            task_id: "T1".into(),
            object_name: "斯迪克".into(),
            period: "2026Q4".into(),
            verdict: verdict.into(),
            reviewer: req.reviewer.clone(),
            comment: String::new(),
            revised: verdict == "修改后通过",
            receipt: serde_json::to_value(receipt()).unwrap(),
            review_id: "RV-1".into(),
            status_after: "SUCCEEDED".into(),
            trace_id: "PR-EV-1".into(),
            guardrail_tokens: tokens.iter().map(|s| s.to_string()).collect(),
            original_inference: orig.map(String::from),
            revised_inference: rev.map(String::from),
        }
    }

    #[test]
    fn hallucinated_number_classification() {
        // The real 1652万 case: flagged token disappears in the revision.
        let orig = "风险点：净利率约2.7%，盈利偏薄；扣非低于归母约1652万元。";
        let rev = "风险点：净利率约2.7%，盈利偏薄；扣非净利润5042.06万元。";
        assert_eq!(
            classify_revision(orig, rev, &["1652".to_string()]),
            ErrorCategory::HallucinatedNumber
        );
    }

    #[test]
    fn stale_data_classification() {
        let orig = "Q3单季增速明显回落，2026Q3 报告显示。";
        let rev = "Q4单季增速明显回落，2026Q4 报告显示。";
        assert_eq!(
            classify_revision(orig, rev, &[]),
            ErrorCategory::StaleData
        );
    }

    #[test]
    fn arithmetic_classification() {
        let orig = "前三季营收24.9亿元。";
        let rev = "前三季营收24.83亿元。";
        assert_eq!(classify_revision(orig, rev, &[]), ErrorCategory::Arithmetic);
    }

    #[test]
    fn wording_classification() {
        let orig = "盈利偏薄，现金流承压。";
        let rev = "盈利偏薄，经营现金流明显承压。";
        assert_eq!(classify_revision(orig, rev, &[]), ErrorCategory::Wording);
    }

    #[test]
    fn summary_stats() {
        let records = vec![
            rec("通过", &[], None, None),
            rec("通过", &[], None, None),
            rec("修改后通过", &["1652"], Some("扣非低于归母约1652万元"), Some("扣非5042.06万元")),
            rec("驳回", &[], None, None),
        ];
        let s = analyze(&records);
        assert_eq!(s.total_reviews, 4);
        assert_eq!(s.approves, 2);
        assert_eq!(s.revisions, 1);
        assert_eq!(s.rejects, 1);
        assert!((s.first_pass_rate - 0.5).abs() < 1e-9);
        assert!((s.revision_rate - 0.25).abs() < 1e-9);
        assert_eq!(s.guardrail_flagged, 1);
        assert_eq!(s.guardrail_confirmed, 1);
        assert_eq!(s.top_tokens, vec![("1652".to_string(), 1)]);
        assert_eq!(s.taxonomy.iter().find(|(c, _)| *c == ErrorCategory::HallucinatedNumber).unwrap().1, 1);
        assert_eq!(s.per_company.len(), 1);
        assert_eq!(s.per_company[0].reviews, 4);
    }

    #[test]
    fn empty_log_is_all_zero() {
        let s = analyze(&[]);
        assert_eq!(s.total_reviews, 0);
        assert_eq!(s.first_pass_rate, 0.0);
        assert!(s.per_company.is_empty());
    }
}
