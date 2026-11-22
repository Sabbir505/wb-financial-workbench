//! Evaluation harness for the planted-violation corpus.
//!
//! The corpus is generated from upstream fixtures by `workbench/eval/generate_corpus.py`.
//! Evidence facts come from the announcement excerpt and deterministic metrics only;
//! the brief's own CALCULATION section is never passed to the checker.
//!
//! Run: cargo test --offline -p wb-e2e --test eval_corpus -- --nocapture

use serde_json::Value;
use wb_core::binding::{facts_from_sources, BindingVerdict, FactSources};
use wb_core::guardrail::{check_with_facts, GuardrailConfig};
use wb_core::model::Evidence;

#[derive(Clone, Copy, Default)]
struct Counts {
    tp: usize,
    fp: usize,
    fn_: usize,
    tn: usize,
    advisory_hits: usize,
    advisory_total: usize,
}

#[test]
fn planted_corpus_meets_the_acceptance_criterion() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../eval/planted-corpus-v1.json"
    );
    let text = std::fs::read_to_string(path).expect("corpus JSON");
    let root: Value = serde_json::from_str(&text).expect("valid corpus JSON");
    let cases = root["cases"].as_array().expect("cases array");
    assert!(cases.len() >= 50, "corpus too small: {}", cases.len());

    let cfg = GuardrailConfig::default();
    let mut failures = Vec::new();
    let mut overall = Counts::default();
    let mut per_condition: std::collections::BTreeMap<String, Counts> =
        std::collections::BTreeMap::new();

    for case in cases {
        let id = case["id"].as_str().unwrap_or("?");
        let condition = case["condition"].as_str().unwrap_or("?").to_string();
        let inference = case["inference"].as_str().unwrap_or("");
        let evidence: Evidence = serde_json::from_value(case["evidence"].clone())
            .unwrap_or_else(|e| panic!("{id}: evidence: {e}"));
        let label = &case["labels"][0];
        let raw = label["raw"].as_str().unwrap_or("");
        let value_exp = label["value"].as_str().unwrap_or("");
        let binding_exp = label["binding"].as_str().unwrap_or("");

        // No metrics, no CALC: structured evidence and its excerpt only.
        let facts = facts_from_sources(FactSources {
            evidence: Some(&evidence),
            ..Default::default()
        });
        let g = check_with_facts(inference, &facts, &cfg);

        let flagged = g.violations.iter().any(|v| v.raw == raw);
        let binding = g.bindings.iter().find(|b| b.raw == raw);
        let verdict = binding.map(|b| &b.verdict);
        let is_mismatch = matches!(verdict, Some(BindingVerdict::Mismatch { .. }));
        let is_unsupported = matches!(verdict, Some(BindingVerdict::Unsupported));

        let expected_flag = value_exp == "flag";
        if flagged != expected_flag {
            failures.push(format!(
                "{id}: value expected {value_exp}, got {} (violations {:?})",
                if flagged { "flag" } else { "pass" },
                g.violation_tokens()
            ));
        }
        let binding_ok = match binding_exp {
            "ok" => !is_mismatch && !is_unsupported && verdict.is_some(),
            "mismatch" => is_mismatch,
            "unsupported" => is_unsupported,
            other => panic!("{id}: unknown binding label {other}"),
        };
        if !binding_ok {
            failures.push(format!("{id}: binding expected {binding_exp}, got {verdict:?}"));
        }

        let c = per_condition.entry(condition).or_default();
        match (expected_flag, flagged) {
            (true, true) => {
                c.tp += 1;
                overall.tp += 1;
            }
            (true, false) => {
                c.fn_ += 1;
                overall.fn_ += 1;
            }
            (false, true) => {
                c.fp += 1;
                overall.fp += 1;
            }
            (false, false) => {
                c.tn += 1;
                overall.tn += 1;
            }
        }
        if binding_exp == "mismatch" {
            c.advisory_total += 1;
            overall.advisory_total += 1;
            if is_mismatch {
                c.advisory_hits += 1;
                overall.advisory_hits += 1;
            }
        }
    }

    println!("condition              TP  FP  FN  TN  advisory");
    for (name, c) in &per_condition {
        println!(
            "{name:22} {:>2}  {:>2}  {:>2}  {:>2}   {}/{}",
            c.tp, c.fp, c.fn_, c.tn, c.advisory_hits, c.advisory_total
        );
    }
    let recall = overall.tp as f64 / (overall.tp + overall.fn_) as f64;
    println!(
        "TOTAL                  {:>2}  {:>2}  {:>2}  {:>2}   {}/{}",
        overall.tp,
        overall.fp,
        overall.fn_,
        overall.tn,
        overall.advisory_hits,
        overall.advisory_total
    );
    println!("value recall = {:.1}%", recall * 100.0);

    assert!(failures.is_empty(), "label mismatches:\n{}", failures.join("\n"));
    assert!(overall.fp == 0, "false blocks on clean text: {}", overall.fp);
    assert!(recall >= 0.90, "invented-number recall below 90%: {recall:.3}");
}
