# Spec conformance — professor's 4-part requirements vs. repo

Governing document: supervisor spec, 2026-11-18 (4 sections: knowledge base, guardrail+workflow,
system design, acceptance). This file is the requirement-by-requirement audit. Update it whenever
a requirement changes state. Every status here is checked against the repo, not against intent.

Status legend: **DONE** (built + tested) · **PARTIAL** (built, deviates or incomplete) ·
**NOT STARTED** · **BLOCKED** (needs something else first).

Last audited: 2026-11-18. Test baseline: 59 tests green (17 wb-app, 29 wb-core, 13 wb-e2e).

---

## 1. Construction of the financial knowledge base

| # | Requirement | Status | Evidence / gap |
|---|---|---|---|
| 1.1 | Ontology entities: Company, Event, Evidence, ResearchTask, ContextBundle, ReviewAction, versioned Artifact | **PARTIAL** | No ontology artifact exists (no OWL/RDF/schema file; `search_content` for `ontology` = 0 hits). Types exist implicitly in contract v3: ResearchTask/ContextBundle/Evidence as structs, Artifact as the vault brief, ReviewAction as the audit record, Company as `object_id`, Event as `event_id`. Never named, typed or documented as one model. |
| 1.2 | Relations: event→task, task→evidence, brief lineage v1→v2, review state transitions, append-only audit chain | **PARTIAL** | State transitions **DONE** (`wb-core/src/state.rs:1-3`, `:63-93`). Lineage **DONE** (F6 writes `{task_id}.v2.md` and retains `.v1.md`). Audit chain **DONE**, append-only JSONL (`wb-core/src/audit.rs:53-64`). event→task and task→evidence exist only as JSON fields, not as queryable relations. |
| 1.3 | Collect from public sources (cninfo, interactive Q&A, periodic reports); extract entities+relations; store in embedded **SQLite** (Neo4j for comparison) | **OUT OF SCOPE (student)** | No SQLite dependency anywhere: `search_content` for `sqlite\|rusqlite` across 119 files = **0 matches**. Storage is versioned Markdown vault + JSONL. Collection is done by the external research service; the repo has no ETL and no extractor. | **Supervisor owns this side as of 2026-11-18** — kept here as the contract requirement, not a student build task.
| 1.4 | Versioned Markdown briefs, fixed sections FACT / CALCULATION / INFERENCE / CHECK | **DONE** | `wb-core/src/vault.rs`: 13 frontmatter fields, 4 sections, `{vault}/research/{yyyy-mm}/{task_id}.md`. e2e `:196-212`. |

## 2. Numeric guardrail and review workflow

| # | Requirement | Status | Evidence / gap |
|---|---|---|---|
| 2.1 | Deterministic guardrail: evidence set from metrics + announcement excerpts, extract every number claim, 1% relative / **0.02pp for percentages**, red-flag untraceable, model-computed numbers not accepted as evidence | **PARTIAL** | Built: `wb-core/src/guardrail.rs:98-208`, wired at `wb-app/src/workspace.rs:442-449`. **Deviation:** `within_tolerance` takes `_is_pct` and never uses it (`guardrail.rs:210`), so the 0.02 absolute floor applies to **all** numbers, not just percentages. This is now a spec non-conformance, not a code smell. |
| 2.2 | Review workflow: approve / approve-as-revised (live diff, original preserved, version bump) / reject; idempotent submit with 409; local audit log + JSON export | **DONE** | `wb-core/src/diff.rs` (F6), `.v1.md` retention asserted in e2e `:390-410`; 409 path in `conflict_on_failed_task_and_unknown_object_404`; `audit.rs:53-64` + JSON export in the audit view. |
| 2.3 | Task priority P1–P4 = importance × urgency, deterministic rules | **PARTIAL** | `wb-core/src/state.rs:82-93` `quadrant_of()`; urgency is deterministic (`updated_at < 24h`), but **importance is read from the upstream `priority` string** (`wb-mock/src/lib.rs:171-199`), not computed by the workbench. Deterministic, but not self-derived. |
| 2.4a | Guardrail on vs off, **50–100 labeled samples**: intercept rate of invented numbers + false-positive rate on clean text | **NOT STARTED** | Corpus is 5 task fixtures / 4 reviewable / 2 violations (斯迪克 1652万, 永冠 2460万). Nothing labeled, nothing counted. **This is the thesis spine and the only blocking experiment.** |
| 2.4b | Quadrant ordering vs linear FIFO on review-cycle time | **NOT STARTED** | Ordering exists (`state.rs`), no comparator, no timing, no FIFO baseline. |
| 2.4c | Replay six scenarios: normal, duplicate, correction, missing data, out-of-scope, injected numeric error | **PARTIAL** | Fixtures exist for each: normal/`analysis_task.*`, duplicate (idempotent replay + 409), correction (`revise_flow_*`), missing data (`analysis_task.waiting_data.json` → WAITING_DATA), out-of-scope (`analysis_task.ignored.json` → IGNORED), injected error (1652万 / 2460万). e2e covers 6 tests but there is **no replay harness that emits a result table** — that is what 2.4c asks for. |

## 3. System design and implementation

| # | Requirement | Status | Evidence / gap |
|---|---|---|---|
| 3.1 | Windows-native Rust+GPUI app: inbox with quadrant view, data-cutoff badge, red-flag tooltips, review modal with live diff, audit view, Windows menubar + context menus, CJK IME, selection/cut/copy/paste | **DONE** | `wb-app` (GPUI per `wb-app/Cargo.toml:12`), 17 tests; `workbench/screenshots/` has i18n-en/zh, modal, quadrant, audit captures. |
| 3.2 | Backend: local research service, three contract channels I1 webhook / I2 vault / I3 MCP, read-only REST for announcements, metrics, evidence | **DONE** | `wb-mock/src/lib.rs`; `wb-core/src/research.rs` (I3 tools/call + I4/I6 reads, structured 404/409/503/400); I1 listener idempotent. |
| 3.3 | Data layer: **SQLite** + versioned Markdown vault + JSONL audit + mock with real-run fixtures for offline dev | **PARTIAL** | Vault **DONE**, JSONL **DONE**, mock **DONE** (vendored `Ref+Fun+MockData/api_mock/`). **SQLite absent — supervisor's side** (see 1.3; the workbench's persistence is the Markdown vault + JSONL audit, which is what I2/I3 require). |

## 4. Acceptance

| # | Criterion | Status | Note |
|---|---|---|---|
| 4.1 | Covers all F1–F12 | **DONE** | F1–F12 traceable in `THESIS_INDEX.md`. F13 analytics + F14 local chat are **beyond** spec and untracked in git — keep them out of the acceptance claim. |
| 4.2 | Guardrail intercepts **≥90%** of invented numbers with **zero false blocks on clean text** | **NOT MEASURED** | Blocked entirely on 2.4a. Note the criterion is stricter than a precision target: on a clean brief, *no* number may be flagged. Main false-positive risk is an incomplete/truncated evidence excerpt, not the tolerance. |
| 4.3 | event → brief → review → audit loop fully reproducible offline | **DONE** | 59 tests; `--selfcheck-e2e` drives the real UI to SUCCEEDED at v2. |
| 4.4 | Deliverables: thesis, runnable demo, **privacy-scanned** fixture dataset | **PARTIAL** | Demo **DONE**. Privacy scan **DONE** — `Ref+Fun+MockData/api_mock/check_privacy.py` exists and passes (61 entries, 0 hits, verified 2026-11-18); needs its report committed. Thesis **not started**. |

---

## What this audit changes

1. **Q-01 is answered by the supervisor.** Acceptance 4.2 fixes the main claim: *the guardrail
   intercepts ≥90% of invented numbers with zero false blocks on clean text.* That is mechanical
   and needs **no human subjects**, so the ethics-approval bottleneck on the thesis spine is gone.
   Reviewer-detection / automation-bias work becomes optional extension, not the core.
2. **The thesis genre is now fixed** by the title: "Design and Implementation of …" — a design-and-
   implementation DSR thesis whose evaluation is three comparative experiments, not a hypothesis
   test about human reviewers.
3. ~~**New required scope: ontology + SQLite (1.1–1.3).**~~ **RESOLVED 2026-11-18 — the supervisor
   owns the ontology database.** The student owns the Local AI Workbench; the two sides interact over
   the API (I1–I9). Consequence: §1.1–1.3 are no longer the student's build task, but the **contract
   becomes a deliverable in its own right** — it is the boundary between two people's work, so it must
   be documented precisely in Ch.4 and the mock must be presented as the reference implementation of
   the service side, not as scaffolding. The §3c ontology table in `THESIS_INDEX.md` still stands as
   the student's half: the seven entity shapes the workbench consumes. Ask the supervisor for the
   service's schema so the two halves can be checked against each other.
4. **2.4a is the single blocking item.** 50–100 labeled samples, then two numbers (intercept rate,
   false-positive rate). Everything else in the thesis waits behind it.

## Open defect list (code-level, affects evaluation)

| ID | Defect | Location | Effect |
|---|---|---|---|
| D-01 | `_is_pct` ignored — 0.02 absolute floor applies to all numbers | `wb-core/src/guardrail.rs:210` | Spec 2.1 non-conformance; small numbers near tolerance can pass wrongly |
| D-02 | Offline path uses `Default::default()` tolerances instead of the service's methodology | `wb-app/src/workspace.rs:471-486` vs `:434-441` | Same brief can flag differently online vs offline — breaks determinism claim |
| D-03 | `AuditRecord` stores no evidence digest | `wb-core/src/audit.rs:11-38` | Flags cannot be reproduced after the evidence packet changes |
| D-04 | Evidence is synthesized from the brief's own FACT section for 4 of 5 briefs | `wb-mock/src/lib.rs:144-150` | **Circular** — any metric computed on those briefs is inflated by construction |
| D-05 | Upstream `check` strings contradict their own content (斯迪克 says 数字校验通过, contains 1652万) | `api_mock/analysis_task.response.json:29-30` | Cannot be used as labels; hand adjudication required |
| D-06 | `analysis_task.jizhi.json` duplicates 德冠新材's fact block under a different ticker | `api_mock/` | Corpus contamination |
