# THESIS_STATE — WB thesis working memory

Maintained by the WB Thesis Mentor bot. Read this first; update it after substantive work.
Do not store claims here that aren't verified against the repo or a primary source.

**Master index → `THESIS_INDEX.md`** (repo map, spec→code→test traceability, evidence/defect/metric/
task registers). This file stays short: scope, status, decisions, open items.

Last updated: 2026-11-18

**Spec conformance → `THESIS_SPEC_CONFORMANCE.md`** (supervisor's 4-part spec, requirement by
requirement, with status + gap + code refs). Audited 2026-11-18.

## Scope lock

- Thesis subject: **WB — financial-report event-driven analysis & review workbench**
- Discipline/claim: **"the model never decides"** — every number in model-generated text must
  trace to an evidence number; untraceable numbers are red-flagged before human approval.
- Repo: `D:\projects\thesis` (workbench/ = Rust workspace; Ref+Fun+MockData/ = spec v3 + mocks)
- In scope: framing, RQs, literature, evaluation design + execution, chapters, supervisor
  feedback, and repo work that serves the thesis (eval harness, fixtures, figures).
- Out of scope: features beyond F1–F12, stock-price/prediction work, unrelated coding,
  CVs/job applications, other projects (Relay, Velox Studio).

## Artifact status (verified against repo)

- Crates: `wb-core` (contract v3 models, I2 vault parser, F4 guardrail, F6 diff, F7 audit,
  F8 MCP client, I1 webhook) · `wb-mock` (stand-in research service) · `wb-app` (GPUI desktop
  UI, bilingual EN/中文) · `wb-e2e` (end-to-end over real HTTP).
- Guardrail (deterministic, no LLM in loop): 万/亿 scale-aware numeric extraction,
  1% relative / 0.02pp absolute tolerance; differences and computed ratios never count as
  evidence. (`wb-core/src/guardrail.rs`)
- Tests (re-run 2026-11-10, all green): 17 wb-app + 29 wb-core + 13 wb-e2e = **59**;
  `--selfcheck-e2e` drives the real UI through approve → revise → MCP submit and asserts
  SUCCEEDED at brief v2.
- Git: 3 commits; latest `0a71058` (2026-11-06). **Tree is dirty**: F13 analytics, F14 local chat,
  structural layer 2 and `e2e_chat_pipeline` are untracked, 13 files modified. The V1.0 zip the
  supervisor saw contains none of those — freeze the artifact before any evaluation claim.
- Seed anomaly case: 斯迪克 2026Q3, model wrote 扣非低于归母约 1652万 — its own subtraction,
  absent from evidence; guardrail flags it, corrected rewrites pass.

## Decisions

| Date | Decision | Rationale |
|---|---|---|
| 2026-11-02 | Function list final-v2: F1–F12, P0 = review closed loop | Thesis = closed loop, not the KG portal |
| 2026-11-06 | Adopt professor's V1.0 revisions | Supervisor feedback round 1 |
| 2026-11-18 | **Q-01 CLOSED.** Main claim fixed by supervisor acceptance 4.2: guardrail intercepts ≥90% of invented numbers with zero false blocks on clean text | Supervisor spec §4; mechanical, no human subjects required |
| 2026-11-18 | Thesis genre fixed: "Design and Implementation of a Local AI Workbench Driven by Financial Report Events" — design-and-implementation DSR, 3 comparative experiments (§2.4) | Supervisor title |
| 2026-11-18 | ~~Scope widened by supervisor: ontology + embedded SQLite are now required~~ **SUPERSEDED the same day** — see next row. The spec asks for them; the supervisor then assigned the ontology DB to their own side. | Supervisor spec §1.1–1.3, then clarified by message |
| 2026-11-18 | **Scope split confirmed by supervisor.** The ontology database is the *supervisor's* deliverable; the student owns the Local AI Workbench, the two sides interact over the API (I1–I9). The AI part is the agent's processing of received information (sorting, matching); manual review is one scenario, others permitted. | Supervisor message, 2026-11-18. Supersedes the §1.3 SQLite negotiation — **T-20/T-19 in the index now frame it as a contract-boundary question, not a build decision.** |
| 2026-11-18 | F13 analytics + F14 chat **un-scoped** (D10 withdrawn); four advisory AI modules added: provenance (evidence matching), duplicate/restatement detection, pre-review summary, triage ordering | Same supervisor message; see `THESIS_INDEX.md` §3d. All four are read-only — none decides, so the central claim is untouched. |

## Open items

- [x] ~~One research question~~ — CLOSED 2026-11-18 by supervisor acceptance §4.2.
- [ ] **Build the 50–100 labeled sample corpus (§2.4a).** Single blocking item for Ch.6.
      Needs: real evidence packs (fix D-04 circularity first), hand-adjudicated labels
      (D-05 labels are unusable), planted violations incl. near-misses outside 1%.
- [x] ~~Fix D-01 (`_is_pct` unused)~~ — **DONE 2026-11-18**, two regimes, tests pin both. Flag set changed.
- [ ] Negotiate ontology + SQLite scope (§1.1–1.3): designed artifact vs runnable store.
- [x] Privacy scan verified 2026-11-18: `api_mock/check_privacy.py` passes (61 entries, 0 hits). Only the committed report remains (T-16).
- [ ] Experiment §2.4b: quadrant ordering vs FIFO on review-cycle time (no harness yet).
- [ ] Experiment §2.4c: six-scenario replay **harness emitting a result table** (fixtures exist,
      harness does not).
- [ ] Freeze the artifact: commit the uncommitted extension work, tag `thesis-eval-v1` (T-01).
- [ ] Fixed dataset of briefs with **planted** numeric violations (needed for any metric).
- [ ] Evaluation conditions + metrics defined (guardrail P/R, reviewer detection rate,
      review time, correction accuracy, audit completeness).
- [ ] Ethics approval decision if any human-subject study is planned (before recruiting).
- [ ] Literature: groundedness/attribution, numeric errors in LLMs, HITL review & automation
      bias, financial-report NLP, DSR method — real citations only.
- [ ] Chapter skeleton mapped onto existing repo docs.

## Chapter status

| Chapter | Status | Source material in repo |
|---|---|---|
| 1 Problem / intro | not started | README "Why this exists", 1652万 case |
| 2 Related work | not started | — (needs real citations) |
| 3 Method (DSR) | not started | — |
| 4 System design | **draft started** — `chapters/04-knowledge-model.md` | WB-KG-Interface-Contract-v3.md (I1–I9); ontology §1.1–1.2 written as §4.1–4.4 |
| 5 Implementation | draftable now | workbench/ crates, F1–F12 table |
| 6 Evaluation | blocked | needs dataset + metrics |
| 7 Discussion / validity | not started | README known limitations |
| 8 Conclusion | not started | — |

## Supervisor feedback log

| Date | Item | Status |
|---|---|---|
| 2026-11-06 | V1.0 revision (IME input, light theme, menu bar, privacy gating, expanded mocks) | applied in `0a71058` |

## Session log

| Date | Note | Summary |
|---|---|---|
| 2026-11-18 | `THESIS_SPEC_CONFORMANCE.md` | Supervisor's 4-part spec audited requirement by requirement. Q-01 closed by acceptance §4.2. Ontology + SQLite verified absent (0 hits for `sqlite`/`ontology` across 119 files). 11 DONE, 6 PARTIAL, 4 NOT STARTED, 1 NOT MEASURED. |
| 2026-11-18 | `chapters/04-knowledge-model.md` | Chapter 4 draft written: 7 entities → Rust types with field lists and line refs, 6 relations (R4/R5 real, R2/R3 indirect, R6 append-only but **not** a chain), ER diagram, cardinalities, persistence table, and a §4.5 that states the three non-conformances (no SQLite, no hash chain, no graph query) rather than hiding them. Verified `event_ref` carries `EV-*` ids in all 5 briefs. Index §3c + T-10/T-11 added. |
| 2026-11-18 | `THESIS_INDEX.md` §3b | F1–F12 conformance audit against the function list. **No function absent; 5 partial** (F3 source_version unrendered, F4 no evidence-value tooltip + `_is_pct` ignored, F9 toast not a reminder, F11 metrics never rendered and `/financial/reports|indicators` never called, F12 priority read from fixtures — no deterministic rule). §2.3 priority model has no implementation, so §2.4b cannot be run as written. §2.4c: 4 of 6 scenarios have e2e coverage; missing-data and out-of-scope do not. |
-11-10 | `notes/2026-11-10-session.md` | Repo explored end to end; 59 tests re-verified; `THESIS_INDEX.md` created; framing corrected (traceability ≠ correctness, no self-clean step, metrics are per-number not per-report); provenance traced, circular evidence found on 4 of 5 briefs. No RQ chosen. |
