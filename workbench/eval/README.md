# Planted-violation corpus, v1

Generated 2026-11-22 by `generate_corpus.py` from upstream fixtures in
`Ref+Fun+MockData/api_mock`:

- `financial_reports.*.json` supplies the announcement excerpt, which is the
  upstream evidence text.
- `financial_indicators.*.json` supplies deterministic metrics.

The brief's own CALCULATION section is never used as evidence. The harness
(`crates/wb-e2e/tests/eval_corpus.rs`) passes only the evidence pack to
`facts_from_sources`, so a value check cannot verify the brief against itself.

## Regenerate and run

```
python workbench/eval/generate_corpus.py
cargo test --offline -p wb-e2e --test eval_corpus -- --nocapture
```

## Structure

56 cases from 8 base tasks. Each case is one short INFERENCE sentence, one
evidence pack with structured facts, and one label:

- `value`: `pass` or `flag` (the supervisor's red verdict).
- `binding`: `ok`, `mismatch`, or `unsupported` (advisory).

Seven conditions, eight cases each: `clean_amount`, `invented_difference`,
`near_miss_outside` (2% above the evidence amount), `within_tolerance` (0.5%),
`wrong_account`, `wrong_period`, `wrong_scope`.

## Result, 2026-11-22

| Condition | TP | FP | FN | TN | Advisory |
|---|---:|---:|---:|---:|---:|
| clean_amount | 0 | 0 | 0 | 8 | 0/0 |
| invented_difference | 8 | 0 | 0 | 0 | 0/0 |
| near_miss_outside | 8 | 0 | 0 | 0 | 0/0 |
| within_tolerance | 0 | 0 | 0 | 8 | 0/0 |
| wrong_account | 0 | 0 | 0 | 8 | 8/8 |
| wrong_period | 0 | 0 | 0 | 8 | 8/8 |
| wrong_scope | 0 | 0 | 0 | 8 | 8/8 |
| **Total** | **16** | **0** | **0** | **40** | **24/24** |

Value recall on planted invented numbers: 100%. False blocks on clean text: 0.

## What this result is and is not

This is a deterministic regression oracle for the checker, not a measured
performance claim about the research agent. The violations were written by the
same person who wrote the checker, from a small set of fixtures, and the cases
share evidence packs in groups of seven. The effective sample for any
confidence interval is **8 base tasks**, not 56 independent observations.

Do not report "100% precision" from this table. Report: "the checker passes a
56-case planted-oracle suite built from eight fixture announcements, with zero
false blocks on clean controls." A claim about real model output needs briefs
written by the upstream agent and labels adjudicated independently.
