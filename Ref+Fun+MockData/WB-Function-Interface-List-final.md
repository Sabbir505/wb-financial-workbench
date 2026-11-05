# WB Function & Interface List (Final · Aligned with the Original API List)

Version: final-v2 (2026-11-02) · Scope: **WB = a financial-report event-driven analysis &
review workbench** (the "read, trust, review, trace" closed loop for research briefs + graph
evidence queries). **No stock-price / market / money-flow / backtest features** — those stay
on the KG portal (`/stock`) and the review console.
**Interface-form principle: stay as close as possible to the paths and grouping of the original
`financial_workbench_api_list`; add no new interface unless strictly necessary** — every
deviation is justified inline. Base URL follows the original list: `/api/v1`.
Companion docs: `Ref/WB-KG接口契约-v3.md` / `Ref/WB-KG-Interface-Contract-v3.md` (WB-side
channels I1–I3) · `Ref/testdata/api_mock/` (per-endpoint mock data).
Chinese counterpart: `Ref/WB功能与接口清单-final.md` (section numbering matches one-to-one).

## 1. Function List (12 items)

### P0 · Review Closed Loop (full phase-1 / thesis scope)

| # | Function | One-liner | Interface dependency |
|---|---|---|---|
| F1 | Research brief library | Browse & full-text search of briefs in vault `research/` | I2 |
| F2 | Brief detail view | Four-section rendering (FACT / CALCULATION / INFERENCE / CHECK) + frontmatter | I2 |
| F3 | Data-cutoff badge | "data cutoff × source version" pinned on top; turns amber after 3 days | I2 frontmatter |
| F4 | Numeric guardrail red-flagging | Every number in the draft is compared against evidence/tool outputs; misses are red-marked with source tooltips | `calculate` tool + evidence numbers from `/financial/indicators` (**WB-internal, no server endpoint**) |
| F5 | Review inbox | Queue (status/company filters) → detail → approve / approve-as-revised / reject | I3 |
| F6 | Approve-as-revised | Editor + original-draft diff + version bump; original untouched | I3 |
| F7 | Review audit trail | Local-first review log (survives service outages), JSON export | I3 receipts |
| F8 | Generic MCP channel | The single IPC for calling the research service (approval policy + timeout + structured errors) | I3 |
| F9 | New-brief notification | Webhook triggers a WB automation reminder | I1 |
| F10 | Degraded-service display | Queue shows "unavailable" instead of blank when the research service is down | — |

### P1 · Financial-evidence Enhancements

| # | Function | One-liner | Interface dependency |
|---|---|---|---|
| F11 | Graph evidence drill-down | Click a brief's evidence back to the announcement excerpt / metrics / evidence pack | `/financial/reports` `/financial/indicators` `/audit/{id}/evidence` |
| F12 | Event quadrant view | Pending queue ordered by P1–P4 importance×urgency (aligned with review-console rules) | `/analysis/history` |

Deferred: `/query` free-form Q&A (enabled after the guardrail review; restricted to the
financial-evidence domain).
Removed (versus previous revision): stock-price analysis view, prediction/backtest queries,
fundamental-risk event panel (not required by the closed loop; data stays on the KG
portal/review console); I7 HTTP review submit (redundant with I3 — the file contract is the
fallback); I8 server-side numeric check (the guardrail is WB-internal); I9 alerts query.

## 2. Interface List (form aligned with the original API list)

### 2.1 Financial Data APIs

| Original entry | Verdict | Notes & KG-side landing point | Mock |
|---|---|---|---|
| `GET /market/overview` | **Disabled** (market-side) | System overview is covered by `/system/status` | — |
| `GET /market/quotes/{symbol}` | **Disabled** (stock-price) | Stays on the KG portal/review console | — |
| `GET /market/history/{symbol}` | **Disabled** (stock-price) | Same as above | — |
| `GET /stocks/search?q=` | **Adopted** (as-is) | Company-pool search (ticker↔object_id resolution) | `stocks_search.response.json` |
| `GET /financial/reports/{symbol}?period=` | **Adopted** (excerpt-level) | Periodic-report events with excerpts (≈ former I4 announcements) | `financial_reports.response.json` |
| `GET /financial/indicators/{symbol}?period=` | **Adopted** (live on 8012) | Deterministic computed metrics (≈ former I4 metrics) | `financial_indicators.response.json` |

### 2.2 AI Analysis and Prediction APIs

| Original entry | Verdict | Notes & KG-side landing point | Mock |
|---|---|---|---|
| `POST /analysis` | **Not implemented** | Analysis is event-driven (announcement → task state machine); bare external submission would bypass dedup and the audit chain | — |
| `GET /analysis/{id}` | **Adopted** (id = task_id) | Full task fields + four-section brief + history | `analysis_task.response.json` |
| `GET /analysis/history` | **Adopted** | Queue summaries (with status filters) | `analysis_history.response.json` |
| `GET /analysis/{id}/explanation` | **Adopted** | Inference section + numeric-check result (structured) | `analysis_explanation.response.json` |
| `GET /predictions/{symbol}` | **Disabled** (stock-price) | Predictions/backtests stay on the KG review console | — |
| `GET /predictions/{id}` | **Disabled** (stock-price) | Same as above | — |

### 2.3 Interactive AI Query APIs

| Original entry | Verdict | Notes | Mock |
|---|---|---|---|
| `POST /query` | **Reserved (P2)** | Reconciled with the "LLM never decides" discipline: answers are restricted to the financial-evidence domain and every number must pass the guardrail; enabled after review | — |
| `GET /query/history` | **Reserved (P2)** | Enabled together with `/query` | — |
| `GET /query/{id}` | **Reserved (P2)** | Same as above | — |

### 2.4 Audit and Verification APIs

| Original entry | Verdict | Notes & KG-side landing point | Mock |
|---|---|---|---|
| `GET /audit/{analysis_id}` | **Adopted** (id = task_id) | State-machine history + audit-chain status | `audit_task.response.json` |
| `GET /audit/{analysis_id}/evidence` | **Adopted** (task→event resolution) | Original text + excerpt + locator + fetch time (≈ former I4 evidence) | `audit_evidence.response.json` |
| `GET /audit/{analysis_id}/performance` | **Disabled** (backtest is stock-price) | Performance = backtest outcomes; stays on the KG review console | — |
| `GET /audit/{analysis_id}/methodology` | **Adopted** | Pipeline steps + workflow_version + data_cutoff + source_version | `audit_methodology.response.json` |

### 2.5 System and API Management

| Original entry | Verdict | Notes | Mock |
|---|---|---|---|
| `GET /health` | **Adopted** | Liveness probe | `system_health.json` |
| `GET /models` | **Adopted** (single-element list) | Single-model system (kimi-k3), honestly returns one entry | `system_models.json` |
| `GET /models/{id}` | **Adopted** | Same model, detail view | `system_models.json` |
| `GET /system/status` | **Adopted** | Store counts / last pipeline runs / pending reviews | `system_status.json` |

### 2.6 WB-side Channels (beyond the original list — **necessary, exactly these three**, from the v2 contract)

| # | Interface | Necessity | Mock |
|---|---|---|---|
| I1 | New-brief notification (HTTP webhook, KG→WB) | WB is a separate local app; pending reviews must be pushable; payload is not consumed, idempotent | `i1_webhook_payload.json` |
| I2 | Brief file contract (vault .md, research writes / WB reads) | The carrier of research assets and full-text indexing; 13 frontmatter fields + four sections | `i2_brief_sample.md` |
| I3 | Research tools ×4 (MCP tools/call) | Review submit / task / context / queue — the write path of the review loop; falls back to the I2 file contract when MCP is unavailable | `i3_*.json` (incl. 409 conflict sample) |

Error conventions (global): 404 object not found / 409 version or state conflict /
503 data missing (caller transitions to WAITING_DATA) / 400 bad parameters.

## 3. Mock Data (`Ref/testdata/api_mock/`)

- One JSON sample per endpoint from §2, **all taken from real runs** (the SDIC 2026Q3 storyline).
- Usage: static hosting (`python -m http.server 8899`) or test-double injection (vitest/Rust).
- `internal/` subdirectory: the retired I7/I8/I9 samples kept as **internal references**
  (the I8 numeric-check case — "deducted profit is ~16.52M below net profit" being blocked —
  serves as the test fixture for WB's internal guardrail; **no longer a server interface**).
