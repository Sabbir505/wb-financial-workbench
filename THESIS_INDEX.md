# THESIS_INDEX — master index for the WB thesis

Purpose: one place to find **what exists, where it is, and whether it is proven**. `THESIS_STATE.md`
stays the short working memory (scope lock, open items); this file is the map + registers.
Nothing goes in here that has not been checked against the repo or a primary source.

Snapshot: **2026-11-10** · repo `D:\projects\thesis` · HEAD `0a71058` (+ uncommitted work, see D1) ·
`cargo test --offline` → **59 tests pass** (17 wb-app, 29 wb-core, 7 wb-e2e chat, 6 wb-e2e review loop).

---

## 1. Register conventions

| Prefix | Register | Meaning |
|---|---|---|
| `E-nn` | Evidence | a verified fact about the artifact, with `path:line` proof |
| `D-nn` | Defect / risk | something that would embarrass the thesis if a examiner found it |
| `Q-nn` | Open question | needs a decision from Sabbir or the supervisor |
| `T-nn` | Task | a unit of work that fits in one sitting |
| `C-nn` | Chapter | thesis chapter with its source material |
| `M-nn` | Metric | evaluation measure with its operational definition |
| `L-nn` | Literature | source to obtain *and verify* (DOI checked) before citing |

Rule: an `E` entry without a `path:line` is not an entry. An `M` entry without a formula and a
data source is a wish, not a metric.

---

## 2. Repo map — what is where

| Path | What it is | Thesis role |
|---|---|---|
| `Ref+Fun+MockData/WB-Function-Interface-List-final.md` | Function list final-v2 (2026-11-02): F1–F12, interface adopt/reject table | Ch.4 requirement baseline; defines "in scope" |
| `Ref+Fun+MockData/WB-KG-Interface-Contract-v3.md` | Interface contract v3 (2026-11-01): channels I1–I9, payload shapes, error codes, state machine | Ch.4 design chapter backbone |
| `Ref+Fun+MockData/api_mock/` | 54 entries of mock payloads; 11 `analysis_task*.json`, 9 indicator sets, 10 report sets, retired `internal/` samples | Evaluation data source (**but see D2–D4**) |
| `workbench/crates/wb-core/` | Domain kernel: models, vault I2 parser, F4 guardrail (3 layers: numeric, structural, provenance), duplicate detection, pre-review summary, task priority/ordering, F6 diff, F7 audit, F8 MCP, F13 analytics, I1 webhook | Ch.5 implementation |
| `workbench/crates/wb-mock/` | Stand-in research service (HTTP + MCP + webhook emitter); loads **5** task fixtures (`lib.rs:167-200`) | Test bed; makes e2e hermetic |
| `workbench/crates/wb-app/` | GPUI desktop UI (bilingual EN/中文), review workspace, detail, diff editor, analytics view, local chat | Ch.5 + all screenshots |
| `workbench/crates/wb-e2e/` | 13 integration tests over real HTTP, real vault files, real webhook | Ch.6 "artifact works" evidence |
| `workbench/research-vault/research/2026-10/` | 6 files = 5 seeded briefs + 1 `.v1.md` backup (F6 proof) | Ch.5/6 figure material |
| `workbench/wb-data/reviews.jsonl` | F7 audit trail, **1 record** | Ch.6 analytics demo (n=1, see D6) |
| `workbench/screenshots/` | 69 PNGs: `d2-*` (6), `final-*` (4), `i18n-*` (3), `ux-*` (6), `v3-*` (8), `live-check`, plus `unslop/` | Figure bank (**unregistered, see D9**) |
| `workbench/smoke-data/*.ps1` | Screenshot/UI automation scripts | How figures were produced; reproducibility note |
| `README.md`, `workbench/README.md` | Overview, closed-loop diagram, run/verify guide, known limitations | Ch.1 + Ch.7 raw text |
| `FinancialWB_V1.0.zip` | Supervisor deliverable snapshot (241 entries: specs + mocks + full Rust source + `vendor/`); contains **no** `structural.rs`, `analytics.rs`, `llm.rs`, `chat.rs` | What the supervisor actually saw on 2026-11-06 — i.e. V1.0 predates the current extension work |

Crate size (Rust, src only): `wb-app` 8616 lines / 9 files · `wb-core` 2955 / 13 · `wb-mock` 1109 / 2 ·
`wb-e2e` 859 / 3. Test counts per file: `chat.rs` 13, `structural.rs` 7, `analytics.rs` 6, `guardrail.rs` 4, `state.rs` 3,
`vault.rs` 3, `diff.rs` 2, `llm.rs` 2, `audit.rs` 1, `webhook.rs` 1, `input.rs` 4, `e2e_chat_pipeline.rs` 7,
`e2e_review_loop.rs` 6.

---

## 3. Spec → code → test traceability

`F#` from the final function list; `I#` from contract v3. "Code" = where it lives, "Proof" = the
test or assertion that would fail if it broke.

| Item | Code | Proof |
|---|---|---|
| F1 brief library | `wb-app/src/workspace.rs` (queue + search), `wb-core/src/vault.rs` | `e2e_review_loop.rs` vault assertions `:196-236` |
| F2 four-section detail | `vault.rs` section parser; `wb-app/src/detail.rs` | `vault::tests::parses_13_frontmatter_fields_and_4_sections`; e2e `:196-236` |
| F3 data-cutoff badge | `wb-app/src/detail.rs:482-500` (amber when age > 72 h) | **no test** — visual only (D11) |
| F4 numeric guardrail | `wb-core/src/guardrail.rs:127-208`; wiring `wb-app/src/workspace.rs:442-449` | `guardrail` unit tests `:232-276`; e2e `:188-194` (斯迪克 → exactly `["1652"]`), `:348-350` (永冠 → `["2460"]`) |
| F4 layer 2 structural | `wb-core/src/structural.rs:1-80` (recompute YoY/margin from CALC) | `structural` 7 unit tests; e2e `f13_f4_structural_and_analytics_pipeline` |
| F5 review inbox | `wb-app/src/workspace.rs`; `wb-core/src/state.rs:1-3` | `system_queue_and_quadrant_contract`; `state` unit tests |
| F6 approve-as-revised + diff | `wb-core/src/diff.rs`; revised-inference path in `wb-app` | `revise_flow_bumps_version_and_clears_violation` `:390-410` (v2 + `.v1.md` kept) |
| F7 audit trail | `wb-core/src/audit.rs:53-64` (JSONL append, survives outage) | `audit` unit test; e2e writes a record |
| F8 MCP single write path | `wb-core/src/research.rs:1-140` (timeout, 409 reconcile, structured errors) | `full_review_loop_approve`; `conflict_on_failed_task_and_unknown_object_404` |
| F9 / I1 webhook | `wb-core/src/webhook.rs`, `wb-mock` emitter | `webhook` unit test; e2e `:237-247` (idempotent single reminder) |
| F10 degraded display | `wb-app/src/workspace.rs` offline path | `degraded_service_surfaces_transport_errors` |
| F11 evidence drill-down | `wb-core/src/research.rs` GETs (I4/I6) | e2e `:171-185` (context, evidence, methodology) |
| F12 quadrant view | `wb-core/src/state.rs:82-93` `quadrant_of()` (importance = P1/P2, urgency = updated < 24 h); rendered in `workspace.rs` | `state::tests::quadrant_placement`; e2e `:139-156` |
| I2 vault file contract | `vault.rs` (13 frontmatter fields, 4 sections, `{vault}/research/{yyyy-mm}/{task_id}.md`) | e2e `:196-212` |
| I3 submit + idempotency | `research.rs`; `wb-mock` review handler (`RV-*` receipts, `409` current_status) | approve + revise e2e paths |
| I7/I8/I9 | **retired** by the function list ("Removed"), still listed as "New" in contract v3 | **see D5 — specs contradict each other** |
| F13 analytics (beyond spec) | `wb-core/src/analytics.rs:100-216`; `wb-app/src/analytics_view.rs` | `analytics` 6 unit tests (`:274-332`) |
| F14 local chat (beyond spec) | `wb-core/src/llm.rs`; `wb-app/src/chat.rs` (13 tests); `chat_port` 8140 in `wb.config.json` | `e2e_chat_pipeline.rs` 7 tests |
| Self-check | `--selfcheck` / `--selfcheck-e2e` flags in `wb-app/src/main.rs` | drives real UI through approve → revise → SUCCEEDED at v2 |

---

## 3b. F1–F12 conformance audit (verified 2026-11-18)

Definition source: `Ref+Fun+MockData/WB-Function-Interface-List-final.md:20-36`.
Verdict: **no function is absent.** Four are partial — the requirement is met in form but a named
sub-requirement has no code path.

> **Update 2026-11-18 (afternoon).** §3d records four new advisory modules (provenance, duplicates,
> pre-review summary, triage). They do **not** change any row below: F11's metrics drawer is still
> empty, F3 still drops `source_version`, F9 still shows a toast rather than a reminder, and F12
> priority is now computed but the quadrant view still reads the wire label. The original rows
> remain the audit-of-record; §3d says what was added and what is still partial.

| # | Requirement (spec words) | Verdict | What is missing / where |
|---|---|---|---|
| F1 | Browse & full-text search of briefs in vault `research/` | **Met** | `vault.rs:156-172` (substring, occurrence-scored, title ×10); library view `workspace.rs:2289`; wired `:255`; status chips `:2359-2361` |
| F2 | Four-section rendering + frontmatter | **Met** | `detail.rs:140-153` renders FACT / CALCULATION / structural / INFERENCE / CHECK; frontmatter KV `:512-516` |
| F3 | "data cutoff × source version" pinned on top; amber after 3 days | **Partial** | `cutoff_badge` takes `_source_version` and never renders it (`detail.rs:482`). Amber >72 h is correct (`:486`). `source_version` only appears in the raw frontmatter list (`:514`), not in the badge |
| F4 | Every number vs evidence; misses red-marked **with source tooltips** | **Partial** | Check runs `guardrail.rs:127-208`; hover tooltip `detail.rs:843-859` shows a generic *reason*, not the evidence value it was compared against — "source tooltip" is unmet. Also `_is_pct` ignored (`:210`), INFERENCE-only (`workspace.rs:442-445`) |
| F5 | Queue (status/company filters) → detail → approve / revise / reject | **Met** | chips `workspace.rs:2028-2030`, `:2103`; company filter `:2164-2175`; actions `detail.rs:398`, `analytics_view.rs:206` |
| F6 | Editor + original-draft diff + version bump; original untouched | **Met** | live diff `detail.rs:1177-1181` via `wb_core::diff`; v2 + `{task_id}.v1.md` retained |
| F7 | Local-first log (survives outages), JSON export | **Met** | `audit.rs:53-64` JSONL append; export `workspace.rs:791` |
| F8 | Single IPC: approval policy + timeout + structured errors | **Met** | `research.rs:46-58` (`ApprovalPolicy`), `:221-229` (write guard), 409 reconcile |
| F9 | Webhook triggers a WB **automation reminder** | **Partial** | Webhook → transient toast only (`workspace.rs:296-310`); no persisted reminder/automation. Payload dedup is correct (`webhook.rs:146-149`) |
| F10 | Queue shows "unavailable", not blank | **Met** | `workspace.rs:2189-2191`; `degraded_service_surfaces_transport_errors` |
| F11 | Drill-down to announcement excerpt / **metrics** / evidence pack via `/financial/reports`, `/financial/indicators`, `/audit/{id}/evidence` | **Partial — largest gap** | Drawer `detail.rs:940-1074` shows announcement title+summary, excerpt+`source_url`, methodology. **Deterministic metrics are never rendered**: `ctx.metrics` is read only at `workspace.rs:424` to build the guardrail receipt list. `/financial/reports` and `/financial/indicators` are never called by `wb-app` (zero matches) — only `/audit/{id}/evidence` and `/methodology` |
| F12 | Queue ordered by P1–P4 **importance × urgency** | **Partial** | `quadrant_of` renders (`state.rs:82-93`, `workspace.rs:2456-2510`) but priority is *read from fixtures* (`model.rs:55`; mock `lib.rs:48`), never computed — no deterministic priority rule exists anywhere. `quadrant_of` derives "important" from the P-string itself, so importance is not an independent axis. Ordering also comes from the **server** (`wb-mock lib.rs:957-961`, priority then updated_at); the app does not re-sort |

### Consequences for the professor's §2.3 and §2.4

- **§2.3 "task priority model … based on deterministic rules"** has no implementation. F12 displays
  priorities that the fixture files assert. Before §2.4b can be run, a rule must exist.
- **§2.4b "quadrant ordering vs linear FIFO"** currently compares two orderings of the *same*
  fixture-assigned priorities, and the app cannot switch orderings — the server decides. Needs a
  sort-mode switch and a computed priority, or the experiment is measuring nothing.
- **§2.4c six scenarios vs the 6 e2e review-loop tests:** normal ✓ (`full_review_loop_approve`),
  duplicate ✓ (`conflict_on_failed_task_and_unknown_object_404`), correction ✓
  (`revise_flow_bumps_version_and_clears_violation`), injected numeric error ✓ (斯迪克 1652, 永冠
  2460), degraded service ✓ (`degraded_service_surfaces_transport_errors`), **missing data ✗**,
  **out-of-scope ✗**. Two of the six required scenarios have no test; `WAITING_DATA` exists only as
  an i18n label (`i18n.rs:625`).

### 3c. Ontology conformance (spec §1.1–1.2) — verified 2026-11-18

Full treatment in `chapters/04-knowledge-model.md`. Summary:

| Spec item | Status | Evidence |
|---|---|---|
| 7 entities (Company, Event, Evidence, ResearchTask, ContextBundle, ReviewAction, Artifact) | **all present as Rust types** | `model.rs:204`, `:187`, `:234`, `:42`+`:152`, `:218`, `:313`+`:325`+`audit.rs:11`, `vault.rs:35` |
| event→task, task→evidence | **indirect** (JSON field / path-addressed) | `Frontmatter.event_ref`; `Evidence.event_id`; `/audit/{task_id}/evidence` |
| brief lineage v1→v2 | **real** | `brief_version`; `{task_id}.v1.md` retained |
| review state transitions | **real** | `state.rs:6-17` (10 states); `HistoryEntry` `model.rs:30-38` |
| append-only audit chain | **append-only, NOT a chain** | `audit.rs:57-60` appends; no `prev_hash`/digest field → **T-10** |
| SQLite + entity extraction + Neo4j (§1.3) | **absent** | zero matches for `sqlite`/`rusqlite` in the workspace; no ETL, no extractor |


### 3d. AI advisory layers — added 2026-11-18

Supervisor instruction of 2026-11-18: the workbench is the artifact, the ontology database is the
supervisor's side, and the AI part is "the LLM agent's basic processing of the received information,
including information sorting, matching", with manual review as one scenario and room for others.
That re-scoped two previously-dismissed features (F13 analytics, F14 chat) from scope creep to
in-scope, and authorised the four advisory modules below.

**The design rule all four obey: none of them decides anything.** They read, they annotate, they
explain. Approval, rejection and revision remain the reviewer's alone, and the guardrail remains the
only component that produces a verdict. Anything that would let an LLM clear a flag would destroy the
central claim, so it was not built.

| Module | Function | What it does | Proof |
|---|---|---|---|
| `wb-core/src/provenance.rs` | F4 layer 3 — evidence matching | Resolves every number in the model text to the *source sentence* that supports it (metric / CALC / announcement excerpt) with the quoted clause and byte offsets | 8 tests; `citation_set_equals_guardrail_violation_set` proves cited ≡ flagged |
| `wb-core/src/duplicate.rs` | Duplicate / restatement detection | Finds other briefs on the same event (`event_ref`), same company+period, or with ≥80% identical CALC figures; reports divergent figures | 11 tests, incl. the 德冠新材 fixture pair and the "small move is not a conflict" case |
| `wb-core/src/presummary.rs` | Pre-review summarisation | What changed since the preserved v1: sections touched, CALC figures moved/added, numbers new to the inference, guardrail verdict delta | 9 tests, incl. the 斯迪克 revise case |
| `wb-app/src/triage.rs` + `wb-core/src/state.rs::order_queue` | Triage / sorting | Computes P1–P4 from the **guardrail verdict** (not the service's self-report) and orders the inbox; FIFO toggle is the §2.4b control | 6 core ordering tests + 6 app tests; `the_two_modes_can_disagree` is the experiment's premise |

**Where they surface:** provenance and the change summary render in both the full page and the drawer
(`detail.rs`); duplicates render after the CHECK section; the inbox ordering and its toggle are in
`workspace.rs`. The chat panel (F14) now receives provenance and duplicates in its system context
(`chat.rs::build_system_context_ext`), so "why is 1652 flagged" is answerable from the real evidence.
`plain_context_is_unchanged_by_the_extended_entry_point` pins that the old prompt is byte-identical
when the advisory slices are empty.

**Two defects found while building these, both fixed:**

1. **`within_tolerance` ignored `_is_pct`** (`guardrail.rs`). The 0.02 floor applied to every number,
   so 亿-scale amounts were compared with a percentage floor — a spec §2.1 violation ("0.02pp for
   percentages"). Now split by regime; `pp_floor_does_not_apply_to_amounts` pins both.
2. **A second revise destroyed the preserved original** (`wb-mock/src/lib.rs`). Service state resets
   to WAITING_REVIEW on restart while the vault keeps the revised file, so the next
   approve-as-revised backed up *already-revised* text over `{task}.v1.md` — silently breaking F6's
   central promise. Found by running the app selfcheck twice against a live mock. The backup write is
   now guarded by `!backup.exists()`; `second_revise_does_not_clobber_the_preserved_original`
   reproduces the bug (verified by reverting the fix and watching the test fail).

**Still partial, unchanged by this work:** F11's drawer never renders `ctx.metrics` (the deterministic
indicators are read once, at `workspace.rs`, only to build the receipt list); F3 drops
`source_version`; F9 shows a transient toast rather than an automation reminder; F12's quadrant view
still reads the wire-supplied priority label rather than the computed one.

**Test count:** 65 → 109 (27 wb-app, 68 wb-core, 7 chat e2e, 7 review-loop e2e). Full suite run twice
consecutively with identical results; `--selfcheck-e2e` passes through the real UI state.

---

## 4. Evidence register (verified 2026-11-10)

| ID | Claim | Proof |
|---|---|---|
| E-01 | Guardrail is deterministic, no LLM call in the path | `wb-core/src/guardrail.rs:1-10`; module has no `llm` import |
| E-02 | Scale-aware extraction: 万/亿 handled via candidate set `v`, `v/1e4`, `v*1e4` | `guardrail.rs:112-122` |
| E-03 | Tolerance = 1 % relative to the evidence value, with a 0.02 absolute floor; the floor applies to **every** number — `within_tolerance` takes `_is_pct` and never reads it, so the percent-vs-absolute distinction is nominal | `guardrail.rs:210-214`; tests `:271-276`; **see D7** |
| E-04 | Only the INFERENCE (model-written) section is checked | `wb-app/src/workspace.rs:442-445` |
| E-05 | Seed case verified end-to-end: 斯迪克 inference contains "1652", guardrail reports exactly one violation token `1652`, approve path still completes | `e2e_review_loop.rs:168`, `:188-194` |
| E-06 | Second case: 永冠 v1 carries "约2460万", flagged; corrected rewrite passes; brief bumps to v2, original kept as `.v1.md` | `e2e_review_loop.rs:343-350`, `:386-405`; file `research-vault/.../S300806.v1.md` exists |
| E-07 | Guardrail flags **only** the invented difference on the 斯迪克 inference — no false positives on the remaining numeric tokens of that text | e2e `:194` asserts `violation_tokens() == ["1652"]` |
| E-08 | Audit trail is local-first JSONL, one line per review, with receipt + both texts | `wb-core/src/audit.rs:53-64`; `wb-data/reviews.jsonl` (1 line) |
| E-09 | State machine RECEIVED → TASK_CREATED → CONTEXT_READY → RUNNING → WAITING_REVIEW → SUCCEEDED, exceptions IGNORED/WAITING_DATA/FAILED/CANCELLED; `is_reviewable()` gates the inbox | `wb-core/src/state.rs:1-3`, `:63-93` |
| E-10 | Mock loads exactly 5 task fixtures: 斯迪克 (P1), 风华 (P1), 洁美 (P2), 永冠 reworked (P2), 立讯 FAILED (P3) | `wb-mock/src/lib.rs:167-200` |
| E-11 | Live reviewable corpus = 4 briefs; violation-bearing = 2 (斯迪克 1652万, 永冠 2460万) | E-10 + e2e assertions |
| E-12 | Upstream `check` field is unreliable as a label: 斯迪克's fixture says "数字校验通过" while its inference contains the 1652万 invention | `analysis_task.response.json:29-30` vs `i2_brief_sample.md:42` |
| E-13 | 59 automated tests pass on 2026-11-10 (`cargo test --offline`) | this session, exit 0 |
| E-14 | F13 analytics exists and classifies revisions into an error taxonomy (arithmetic vs wording) from the audit diff | `wb-core/src/analytics.rs:100-157` |

### Claims the thesis must NOT make yet

- "The guardrail has high precision/recall" — no labelled dataset, 2 positives total (D4).
- "The model never decides" as stated — the model writes the narrative; the guardrail constrains
  **numbers only**, and the human decides verdicts. Restate as: *no numeric claim reaches the
  reviewer's approval screen without an evidence anchor* (D7).
- "The flag improves reviewer accuracy" — no human-subject data exists (D8).

---

## 5. Defects and risks

| ID | Severity | Defect | Action |
|---|---|---|---|
| D1 | **High** | Working tree is dirty — **re-verified 2026-11-18: 27 modified/untracked entries, zero tags, HEAD `0a71058`** (`analytics.rs`, `analytics_view.rs`, `chat.rs`, `llm.rs`, `structural.rs`, `e2e_chat_pipeline.rs` untracked). Nothing reproducible from git. | Commit as one "post-V1.0: F13/F14/structural" commit, tag `thesis-eval-v1`, and cite the tag in every chapter. |
| D2 | High | Fixture set is internally inconsistent: 11 `analysis_task*.json` but the mock loads 5; `analysis_task.jizhi.json` is a duplicate of 德冠新材 (`S300566` vs `S001378`, same fact block); `analysis_history.response.json` says 双星 FAILED ('18.5') while `analysis_task.shuangxing.json` says WAITING_REVIEW/pass, and history is dead data (queue is built from task files). | Declare the corpus explicitly in the thesis (5 loaded / 4 reviewable) and either fix or quarantine unused fixtures. |
| D3 | High | `check` strings are upstream-pipeline artifacts, not WB labels — they disagree with the guardrail (E-12). Any precision/recall computed against them is wrong. | Labels must be hand-adjudicated per number, with a written rubric. |
| D4 | **High (blocks Ch.6)** | Corpus too small: 4 reviewable briefs, 2 violations. No P/R, no detection-rate, no review-time comparison possible. | Build the planted-violation dataset (T-02). |
| D5 | Medium | Spec docs contradict each other: contract v3 lists I7/I8/I9 as "New"; function list final-v2 marks them "Removed" and says the guardrail is WB-internal. | One paragraph in Ch.4 stating which version governs and why; fix the contract doc's status column. |
| D6 | Medium | `wb-data/reviews.jsonl` has 1 record; the F13 analytics screen therefore shows rates computed from n=1. A screenshot of it is a demo, not a finding. | Do not use analytics screenshots as evaluation evidence. Label them "illustrative". |
| D7 | **High (validity)** | Traceability ≠ correctness. The guardrail checks number *presence* within tolerance, not whether the claim uses it correctly; 1 % relative on large values gives wide absolute windows; ×/÷1e4 candidates raise collision probability; the 0.02 floor applies to all numbers; a genuine difference that coincides with an evidence number passes. | Write it into Threats to Validity with the collision estimate; the near-miss study (M-05) exists to measure it. |
| D8 | High | No ethics approval, no protocol, no consent form for reviewer studies. | Start the NJUPT process **before** recruiting (T-05). |
| D9 | Low | 69 screenshots, no register: unclear which commit/scenario each came from. Figures in a thesis must be reproducible. | Figure register (T-08): file → scenario → commit → caption. |
| D10 | ~~Low~~ **Withdrawn 2026-11-18** | ~~`wb-core/src/llm.rs` + `wb-app/src/chat.rs` (F14 local chat) carry 15 tests and a model server config (`chat_port`, `models_dir`) — scope the thesis does not claim.~~ The supervisor put the agent's sorting/matching work in scope; F14 is now a legitimate scenario. | Either a one-paragraph "engineering extension, not evaluated" or drop from the narrative. Do not let it dilute the claim. |
| D11 | Low | F3 badge and several UI affordances have no automated test; UI regressions are only caught by screenshots. | Accept, but state it in Ch.5/7 as a limitation of the verification strategy. |
| D12 | **High (invalidates §2.4a as-is)** | **Circular evidence.** For 4 of 5 briefs the mock synthesizes the evidence pack from the brief's own CALC section (`lib.rs:310-340`) and the metrics from the same (`:729`), and the announcement summary from the brief's FACT section (`:679-686`). Only 斯迪克 (`…-S300806`) has a real pack. Running the guardrail on those four measures the brief against itself: recall → 1.0, FPR → 0.0 by construction. | T-13 before any measurement. Declare the 4 briefs as *not* evaluation-valid in Ch.6. |
| D13 | **High — fixed 2026-11-18** | **A second revise destroyed the preserved original.** Service state resets to WAITING_REVIEW on restart while the vault keeps the revised file, so the next approve-as-revised backed up already-revised text over `{task}.v1.md`, breaking F6's central promise. Found by running `--selfcheck-e2e` twice against a live mock; it also corrupted a tracked fixture in this repo. | Fixed with `!backup.exists()` in `wb-mock/src/lib.rs`; `second_revise_does_not_clobber_the_preserved_original` reproduces it (verified by reverting the fix and watching the test fail). |
| D14 | **High — fixed 2026-11-18** | **`within_tolerance` ignored `_is_pct`** (`guardrail.rs`), applying the 0.02 absolute floor to every number, so 亿-scale amounts were compared against a percentage floor. Spec §2.1 requires 0.02pp *for percentages*. | Fixed by splitting the two regimes; `pp_floor_does_not_apply_to_amounts` pins both, and the 斯迪克 seed case still flags exactly one violation. |

---

## 6. Evaluation design — what is decided, what is not

Status: **no research question chosen, no dataset, no metrics frozen** (as of 2026-11-10).

### Q-01 — research question candidates (Sabbir decides)

| Option | Framing | Needs | Risk |
|---|---|---|---|
| A | Does deterministic numeric traceability checking reduce the share of unverifiable figures reaching approval, compared with (i) LLM self-check and (ii) reviewer-only review? | Planted dataset; 3 conditions; detection rate + time | Human subjects + LLM baseline needed; strongest DSR fit |
| B | Does approve-as-revised with live diff beat reject-and-rewrite on correction accuracy and review time? | Reviewer study, 2 conditions, same briefs | Cheaper, narrower; depends on D8 ethics |
| C | What does a local-first audit trail buy under research-service outage? | Fault injection only, no humans | Cheapest, but thin as a lone RQ; good as a secondary contribution |

Recommended shape (not a decision): **A as the RQ, C as a supporting contribution, B folded into the
task-level analysis of A.** B alone will not fill a chapter.

### M — metric definitions (freeze before running)

| ID | Metric | Operational definition | Source |
|---|---|---|---|
| M-01 | Guardrail recall | flagged planted violations / adjudicated planted violations | planted dataset |
| M-02 | Guardrail precision | true flags / all flags, per number token | planted dataset |
| M-03 | False-positive rate on clean briefs | clean numbers flagged / all clean numbers | planted dataset (near-miss numbers included) |
| M-04 | Reviewer detection rate | briefs where the reviewer names the planted error / briefs with a planted error | reviewer study |
| M-05 | Collision / near-miss sensitivity | detection rate on violations whose value is within 2× tolerance of a real evidence number | planted dataset |
| M-06 | Review time | seconds from opening the task to submitting a verdict; from UI event log | reviewer study |
| M-07 | Correction accuracy | binary: does the submitted revision remove the violation without introducing a new one (guardrail re-run) | audit trail + guardrail |
| M-08 | Audit completeness | required fields present / required fields, per record; survives forced service kill | F7 + fault injection |
| M-09 | Idempotency / reconciliation | duplicate submit → exactly one receipt; 409 path reconciles to current version | e2e + fault injection |

### T — task backlog (one sitting each)

| ID | Task | Definition of done |
|---|---|---|
| T-01 | Freeze the artifact | clean tree, commit, tag `thesis-eval-v1`, `cargo test` green, tag hash recorded here |
| T-02 | Planted-violation dataset v1 | 30–60 briefs derived from real fixture metrics; every number hand-labelled (supported / unsupported / near-miss); label rubric written down; ≥ 10 near-miss cases |
| T-03 | Offline eval harness | one command → guardrail P/R/F1 over the dataset, JSON + CSV out, no UI; reuses `wb-core` (no re-implementation) |
| T-04 | Baselines | (i) LLM self-check prompt on the same briefs, (ii) reviewer-only (flag hidden) — same rubric, same adjudicator |
| T-05 | Ethics | NJUPT application, participant info sheet, consent form; **before** any recruitment |
| T-06 | Reviewer study protocol | task script, counterbalanced conditions, 6–10 participants, time logging |
| T-07 | Fault-injection script | kill the mock mid-review → queue degrades, audit still appends, recovery reconciles (M-08, M-09) |
| T-08 | Figure + screenshot register | every figure: path, scenario, commit, caption, one-line provenance |
| T-09 | Chapter skeletons | C-01…C-08 outlines with the repo doc each section converts (see §7) |
| T-10 | Hash chain for the audit log | `AuditRecord` gains `prev_hash` + `record_hash`; each record commits to its predecessor; unit test proves a mutated middle record fails verification. Makes spec §1.2 "append-only audit chain" literally true. |
| T-11 | ~~`priority_for(importance, urgency)`~~ **DONE 2026-11-18** | `state.rs::priority_for` + `order_queue` + `SortMode`, 11 tests. The inbox now orders app-side with a FIFO toggle, so §2.4b is runnable. **Residual:** the F12 quadrant *view* still groups by the wire-supplied `priority` label (`workspace.rs`, `quadrant_of`) rather than the computed one — same number in practice, but say which is authoritative. |
| T-12 | ~~Fix `_is_pct` in `within_tolerance`~~ **DONE 2026-11-18** | Two regimes; `pp_floor_does_not_apply_to_amounts` pins both. 斯迪克 still flags exactly `1652`. **The flag set changed — any P/R measured before this date is invalid.** |
| T-13 | Independent evidence packs for all reviewable briefs | 4 of 5 briefs synthesize evidence from their own CALC section (`wb-mock/src/lib.rs:144-150`, `:310-340`) and metrics from the brief itself (`:729`). Until each brief has a real raw-yuan pack (斯迪克 shape), P/R is circular. |
| T-14 | Load the missing §2.4c scenarios | **Partial 2026-11-18:** `waiting_data` and `ignored` are now loaded (queue 5 → 7) and `sections: null` deserializes. **Still open:** neither has an e2e replay test, so §2.4c coverage is 4 of 6, not 6 of 6. |
| T-15 | Render deterministic metrics in the F11 drawer | Spec F11 names `/financial/reports` + `/financial/indicators`; the app calls neither. `ctx.metrics` is read only at `workspace.rs:424`. |
| T-16 | Ship the privacy-scan artifact | `check_privacy.py` exists and **passes** (61 entries, 0 hits, run 2026-11-18). Only the report needs committing as the §4.4 deliverable. |
| T-17 | Reconcile the two priority sources | The F12 quadrant view groups by the wire `priority` label while the inbox sorts by the computed one. Make one authoritative (app-computed is the defensible choice) and delete `quadrant_of`'s recency reading. | **Found 2026-11-18** |
| T-18 | Two replay tests for §2.4c | `waiting_data` and `ignored` load but have no e2e test; the spec names six scenarios and coverage is four. |
| T-19 | Decide the revise-write ownership | `wb-mock` writes the vault on revise (`lib.rs:889-916`) although the vault contract (I2) is the service's. Fine for a stand-in, but Ch.4 must say which side owns the write or the F6 claim rests on the mock. |

### Gate ordering — what must precede the experiments

Experiments may not start until Gate 1 is closed. Running §2.4a before T-13 produces recall ≈ 1.0 and
FPR ≈ 0.0 as **artifacts of circular evidence**, not findings.

| Gate | Tasks | Why it blocks |
|---|---|---|
| 0 · Freeze | T-01 | Nothing is reproducible; the tree is dirty, zero tags, HEAD `0a71058`. |
| 1 · Correctness of what is measured | ~~T-12~~ done, **T-13**, ~~T-11~~ done, T-14 (partial), T-03 | Each changes a number the experiment reports. **T-13 is now the only critical item left.** |
| 2 · Dataset | T-02 | No labels, no metric. |
| 3 · Baselines | T-04 | §2.4a is comparative; without baselines it is a single-condition demo. |
| 4 · Human subjects | T-05, T-06 | Only if M-04/M-06 are retained. §2.4b can be simulated instead (no ethics). |
| 5 · Experiments | §2.4a, §2.4b, §2.4c | — |

**Status 2026-11-18:** T-11 and T-12 closed, T-14 half-closed. §2.4b (quadrant vs FIFO) now has
working code on both conditions, so it can run as soon as a queue exists to run it on — it does not
need the dataset, only a seeded arrival stream. §2.4a and §2.4c remain blocked on T-13, T-02, T-04
and the two missing replay tests (T-18).

**Not required before experiments** (writing work, parallel-safe): T-10 hash chain, T-15 F11 metrics,
T-08 figure register, T-09 chapter skeletons, the §1.3 SQLite negotiation.

---

## 7. Chapter plan — convert, don't rewrite

| ID | Chapter | Status | Source to convert |
|---|---|---|---|
| C-01 | Introduction / problem | not started | `README.md` "Why this exists"; 1652万 seed case (`i2_brief_sample.md:42`) |
| C-02 | Related work | not started | nothing in repo — needs L-register |
| C-03 | Method (DSR) | not started | function list (requirements) + contract v3 (design) → DSR artefact cycle |
| C-04 | System design | **draft started** → `chapters/04-knowledge-model.md` | `WB-KG-Interface-Contract-v3.md` I1–I9 + state machine §8; ontology §1.1–1.2 now written as §4.1–4.4; resolve D5 first |
| C-05 | Implementation | draftable now | crate map (§2), F-table (§3), E-01…E-10 |
| C-06 | Evaluation | **blocked on T-02/T-03/T-06** | M-01…M-09 results only |
| C-07 | Discussion / threats to validity | not started | D3, D7, D11 + `workbench/README.md` known limitations |
| C-08 | Conclusion | not started | after C-06; no new claims |

---

## 8. Literature register (obtain, verify DOI, then cite)

Nothing here may be cited until the DOI/URL is fetched and the claim it supports is written next to
it. Status starts at `unverified`.

| ID | Topic | Starting point (verify before use) | Status |
|---|---|---|---|
| L-01 | DSR method | Hevner et al., *Design Science in Information Systems Research*, MIS Quarterly 2004 | unverified |
| L-02 | Hallucination taxonomy | Ji et al., survey of hallucination in NLG, ACM CSUR 2023 | unverified |
| L-03 | Atomic-fact precision | Min et al., FActScore, EMNLP 2023 | unverified |
| L-04 | Retrieval-then-revise attribution | Gao et al., RARR, ACL 2023 | unverified |
| L-05 | Numeric reasoning failures in LLMs | search: arithmetic/table reasoning benchmarks (e.g. FinQA, TAT-QA) — pick 2 with reported numeric-error rates | unverified |
| L-06 | Automation bias / complacency | Parasuraman & Riley 1997; Skitka et al. 1999 — verify venue and exact claim | unverified |
| L-07 | Human-in-the-loop review of AI output | search: human-AI complementarity, over-reliance; pick 2 empirical studies | unverified |
| L-08 | Financial-report NLP | Loughran & McDonald 2011 (textual analysis in finance) + one Chinese A-share disclosure study | unverified |
| L-09 | Provenance / audit trails in regulated AI | search: model documentation, audit log requirements in finance | unverified |

Each entry needs: full citation, DOI, one sentence "what it lets me claim", and where it is cited
(C-0x). Anything not fetched stays out of the text.

---

## 9. Open questions for the supervisor

| ID | Question | Why it matters |
|---|---|---|
| Q-02 | Is a reviewer study with 6–10 fellow students acceptable, or must it be domain reviewers? | Decides M-04/M-06 feasibility |
| Q-03 | Is the Chinese version required in addition to the English one? | Doubles drafting cost — must be known now, not in April |
| Q-04 | ~~Does the F13 analytics / F14 chat work help or dilute the contribution?~~ **ANSWERED 2026-11-18** by the supervisor: the AI part is the agent's processing of received information (sorting, matching), manual review is one scenario, others are allowed. F13/F14 are in scope; D10 is withdrawn. | Scope discipline |

---

## 10. Change log

| Date | Change |
|---|---|
| 2026-11-18 | Supervisor instruction recorded (workbench is the artifact, ontology DB is the supervisor's side, agent sorting/matching is in scope). Four advisory modules added and tested (§3d). D13/D14 fixed with regression tests. Test count 65 → 109; `--selfcheck-e2e` green through the real UI. F13/F14 un-scoped. |
| 2026-11-10 | Index created after full repo exploration; registers E-01…E-14, D-01…D-11, M-01…M-09, T-01…T-09 populated; test suite re-run (59 pass) |
