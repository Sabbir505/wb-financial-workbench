# KG-Palantir × Working Bench Interface Contract (v3 · Merged Edition)

Version: v3.0 (2026-11-01) · Baseline: v2.0 (I1–I5 backbone unchanged) + query-surface expansion
from `financial_workbench_api_list`
Principle: zero changes to the WB kernel; all WB-side adaptation is **config/plugin-level**
(MCP registration, vault directory convention, webhook automation template) and removable at any
time without affecting WB's own upgrade path.
Chinese counterpart: `Ref/WB-KG接口契约-v3.md` (section numbering matches one-to-one).

## 0. Relationship to v2

- **Carried over unchanged**: I1 event notification (webhook), I2 vault file contract,
  I3 MCP review tools, I4/I5 read-only graph queries.
- **New**: I6 extended read-only query surface, I7 review submission (HTTP write path),
  I8 pre-publish numeric guardrail check, I9 trigger-line alerts.
- **Traceability**: Appendix A records the adopt / adapt / reject decision for every entry
  of `financial_workbench_api_list`.

## 1. Interface Overview

| # | Direction | Protocol | Purpose | Status |
|---|---|---|---|---|
| I1 | Graph/Research → WB | HTTP webhook | New-brief / pending-review notification | v2 carried over |
| I2 | Research → WB vault | Filesystem (.md) | Research asset writes (brief/evidence/checks) | v2 carried over |
| I3 | WB → Research | MCP tools/call | Review submit, task/context queries | v2 carried over |
| I4 | WB → KG-Palantir graph | HTTP GET (read-only) | Announcement/metrics/evidence queries | **Implemented** (port 8012) |
| I5 | Research → graph | HTTP GET (read-only) | Context-build pull-back (internal) | **Implemented** (port 8012) |
| I6 | WB → KG-Palantir | HTTP GET (read-only) | Extended surface: market/history/search/predictions/analysis/audit | New |
| I7 | WB → Research | HTTP POST | Review submission (HTTP equivalent of I3; the formal fallback when MCP is unavailable) | New |
| I8 | WB → KG-Palantir | HTTP POST | Numeric guardrail check (pre-publish) | New |
| I9 | WB → KG-Palantir | HTTP GET | Trigger-line alerts (pledge / checklist transitions) | New |

Conventions: `404` = object not found; `409` = version mismatch (response carries the latest
version); `503` = data missing (caller transitions to WAITING_DATA); `422` = numeric check failed
(I8 only); `400` = bad parameters.
Base URL: `http://127.0.0.1:8012/api` (I4–I6, I9 read-only); I7/I8 are hosted by the research
service (write and read paths stay separated).

## 2. Carried-over Interfaces (I1–I3)

Unchanged from v2 §2 and not repeated here: I1 webhook is a pure notification trigger (payload
is not consumed; idempotency guaranteed by the research service); I2 file contract
`{vault_root}/research/{yyyy-mm}/{task_id}.md` with 13 frontmatter fields and four fixed body
sections (FACT / CALCULATION / INFERENCE / CHECK); I3 four MCP tools (submit_review / get_task /
get_context / list_pending).

## 3. I4/I5 Read-only Graph Queries (implemented, port 8012)

| Endpoint | Params | Returns | Notes |
|---|---|---|---|
| `GET /api/announcements/{event_id}` | — | {event_id,title,event_date,object_id,source_url,content_excerpt,object_version} | Announcement info + original source reference |
| `GET /api/companies/{object_id}/metrics` | period*, metrics | {object_id,period,unit,source,as_of,metrics{...}} | Deterministic computed metrics (revenue / net profit / deducted profit / OCF / YoY / margin / reconciliation) |
| `GET /api/evidence/{event_id}` | — | {source_url,excerpt,locator,fetched_at} | Original text + excerpt + locator, with access logging |

## 4. I6 Extended Read-only Query Surface (new; adoption & adaptation of the API list)

> Form principle: the original list is a symbol-centric generic market template; this system is
> **event/task-centric**. Resolution between ticker and object_id is served by `stocks/search`.

| Endpoint | Params | Response essentials | API-list counterpart |
|---|---|---|---|
| `GET /api/market/overview` | — | {date, chains{anchor{events,predictions,backtests}, icg{...}}, pending_reviews, alerts_today} | `/market/overview` |
| `GET /api/market/quotes/{ticker}` | — | {ticker,name,price,pe_ttm,pb,total_mv_yi,float_mv_yi,turnover_pct,holder_num,holder_date,top_holder{name,pct,pledge_pct},hkcc_pct,as_of,source} | `/market/quotes/{symbol}` |
| `GET /api/market/history/{ticker}` | days (default 40) | {ticker, flow[{date,close,pct,turnover_pct,r0_wan,net_wan,amount_yi}], tech{ma5,ma10,ma20,bias5_pct,vol_ratio,rsi6,low20,high20,as_of}} | `/market/history/{symbol}` |
| `GET /api/stocks/search` | q* | [{object_id,name,ticker,exchange,sector,listed}] | `/stocks/search` |
| `GET /api/financial/reports/{object_id}` | period | [{event_id,title,event_date,source_url,content_excerpt,object_version}] (excerpt-level; not a full-text library) | `/financial/reports/{symbol}` (degraded to excerpts) |
| `GET /api/predictions` | object_id | [{prediction_id,event_id,direction,rule_ids,event_tag,created_at,backtest[{horizon,hit,actual_pct,evaluated_at}]}] | `/predictions/{symbol}` |
| `GET /api/predictions/{prediction_id}` | — | single item as above + narrative | `/predictions/{id}` |
| `GET /api/analysis` | status, since | [{task_id,object_id,period,status,updated_at}] | `/analysis/history` |
| `GET /api/analysis/{task_id}` | — | {13 frontmatter fields, sections{fact,calculation,inference,check}} | `/analysis/{id}` |
| `GET /api/analysis/{task_id}/explanation` | — | {inference_text, trend_signal{direction,tag,cohort_hit_rate,own_backtest}, price_impact{valuation,reasoning[],risk_context}} (structured, not HTML) | `/analysis/{id}/explanation` |
| `GET /api/audit/{task_id}` | — | {status, history[{from,to,at,note}], audit_chain{ok,length}} | `/audit/{analysis_id}` |
| `GET /api/audit/{task_id}/evidence` | — | task→event resolution, then same as I4 evidence | `/audit/{analysis_id}/evidence` |
| `GET /api/audit/{task_id}/performance` | — | {backtest[{horizon,hit,actual_pct}], cohort{tag,hits,total,rate}, benchmarks{cum_rate,lift_bayes}} | `/audit/{analysis_id}/performance` |
| `GET /api/audit/{task_id}/methodology` | — | {workflow_version,data_cutoff,source_version,rules[{rule_id,text}],validation{tolerance,checked_at}} | `/audit/{analysis_id}/methodology` |
| `GET /api/health` / `GET /api/system/status` | — | {ok} / {uptime,stores{...},last_pipeline_run} | `/health` `/system/status` |

**Trimmed out**: `/models`, `/models/{id}` from the list are rejected — this system runs a single
model (kimi-k3, with MockLLM as the test double); no multi-model management semantics exist.
The endpoints can be restored if multiple models are ever introduced.

## 5. I7 Review Submission (HTTP POST, write path)

`POST {research_base}/api/review/submit`

| Field | Required | Notes |
|---|---|---|
| task_id | yes | Research task ID |
| verdict | yes | 通过 (approve) / 驳回 (reject) / 修改后通过 (approve-as-revised) |
| comment | no | Review comment |
| reviewer | yes | Reviewer |
| idempotency_key | yes | Idempotency key (recommended: task_id + brief version) |
| revised_content | required when verdict = 修改后通过 | Full revised text (artifacts version bumps) |

Returns `{ok, review_id, status, trace_id}`; `409` if the task is not WAITING_REVIEW (current
status attached). Equivalent to I3 MCP `research.submit_review` — use one or the other; both
write the audit chain.

## 6. I8 Numeric Guardrail Check (HTTP POST)

`POST {research_base}/api/verify/numbers`

| Field | Required | Notes |
|---|---|---|
| text | yes | Text to verify (brief / article draft) |
| task_id | no | When given, evidence numbers are pulled from that task's context |
| evidence_numbers | no | Explicit evidence-number array (mutually exclusive with task_id) |

Returns `{ok, violations[], checked_values[], tolerance_pct}`; HTTP `422` when `violations` is
non-empty. Rule: every numeric claim in the text must trace back to evidence numbers
(deterministic computed metrics + announcement excerpt numbers), default tolerance 1%
(0.02 for percentage points). Same implementation as the review-console numeric check and the
EarningsWorkflow independent validation.

## 7. I9 Trigger-line Alerts (HTTP GET)

`GET /api/alerts?date=YYYYMMDD&kind=pledge_watch|checklist`

Returns `[{kind,title,detail,evidence,emitted_at,source_file}]`. Alerts are emitted immediately
by the daily pipeline (pledge trigger lines / observation-checklist transitions); this endpoint
is read-only.

## 8. Unified Fields & State Machine

- Event fields (end to end): event_id, type, source, object_id, object_version, occurred_at, trace_id
- Idempotency key: source + object_id + object_version + type (contract key); **cross-channel
  duplicates (different source) are deduped by normalized title** — isomorphic to the graph dedup
  key (normalized title + company set + 3-day window)
- Task state machine (six states, phase 1): RECEIVED → TASK_CREATED → CONTEXT_READY → RUNNING →
  WAITING_REVIEW → SUCCEEDED; exception states IGNORED / WAITING_DATA / FAILED / CANCELLED
- Backtest horizons: capital-action/risk events T+1; earnings/transmission events T+5 (primary
  assessment), plus T+20 and CSI-300-relative excess returns

## 9. Boundary Discipline

- Research artifacts never write back into graph facts; model inference must carry source +
  evidence + review status;
- The LLM never decides: direction/hit conclusions come from deterministic rules and backtests;
  the LLM only produces narratives that pass numeric validation;
- I8 is the **pre-publish** check; the audit series is **post-hoc** traceability — neither
  replaces the other;
- All WB-side adaptation is removable at the settings/directory level with one click; WB's own
  upgrades are unaffected;
- Interface changes are backward-compatible by default; incompatible changes go through version
  bump + joint review.

## Appendix A: Traceability of financial_workbench_api_list

| List entry | Decision | v3 landing point / reason |
|---|---|---|
| `/market/overview` | Adopted, adapted | I6 `/api/market/overview` (reworked around the two chains) |
| `/market/quotes/{symbol}` | Adopted | I6 `/api/market/quotes/{ticker}` (Tencent snapshot + valuation + holder structure) |
| `/market/history/{symbol}` | Adopted | I6 `/api/market/history/{ticker}` (daily money flow + technicals) |
| `/stocks/search` | Adopted | I6 `/api/stocks/search` (anchor/chain pool resolution) |
| `/financial/reports/{symbol}` | Adopted, degraded | I6 `/api/financial/reports/{object_id}` (excerpt-level; full-text library is out of scope) |
| `/financial/indicators/{symbol}` | Already implemented | I4 `/api/companies/{object_id}/metrics` |
| `POST /analysis` | Rejected | Analysis is event-driven (announcement → task state machine); no bare external submission |
| `/analysis/{id}` `/analysis/history` | Adopted, adapted | I6 `/api/analysis/{task_id}` `/api/analysis` |
| `/analysis/{id}/explanation` | Adopted, adapted | I6 explanation (structured: inference + trend signal + valuation reasoning) |
| `/predictions/{symbol}` `/predictions/{id}` | Adopted | I6 `/api/predictions*` (with backtest outcomes) |
| `POST /query` | Deferred | Conflicts with the "LLM never decides" discipline; to be re-reviewed after a guardrailed form is designed (query → context bundle + numeric check) |
| `/query/history` `/query/{id}` | Deferred | Same as above |
| `/audit/{id}` | Adopted | I6 `/api/audit/{task_id}` |
| `/audit/{id}/evidence` | Implemented, adapted | I4 evidence + I6 task→event resolution |
| `/audit/{id}/performance` | Adopted | I6 performance (backtest + cohort hit rate + Bayesian lift) |
| `/audit/{id}/methodology` | Adopted | I6 methodology (rule text + versions + data cutoff) |
| `/health` `/system/status` | Adopted | I6 system endpoints |
| `/models` `/models/{id}` | Rejected | Single-model system, no semantic counterpart |
| (missing from list) review write path | **New** | I7 `/api/review/submit` |
| (missing from list) pre-publish numeric check | **New** | I8 `/api/verify/numbers` |
| (missing from list) trigger-line alerts | **New** | I9 `/api/alerts` |
| (missing from list) event notification | Carried over | I1 webhook (v2) |
