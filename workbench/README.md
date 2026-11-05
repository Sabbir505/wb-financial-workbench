# WB 财报审核工作台 (Financial Workbench)

A GPUI desktop app implementing the **WB review workbench** from the thesis
contract (`Ref/WB-KG-Interface-Contract-v3.md` + `Ref/WB-功能与接口清单-final.md`):
the *read → trust → review → trace* closed loop for LLM-generated earnings-report
research briefs, with the discipline that **the model never decides** — every
number in model text must trace back to evidence numbers.

## Workspace layout

| Crate | What it is |
|---|---|
| `crates/wb-core` | Pure logic library: contract-v3 models, I2 vault parser (13 frontmatter fields + FACT/CALCULATION/INFERENCE/CHECK), F4 numeric guardrail, F6 line diff, F7 local audit trail, F8 MCP client (I3 tools/call + I4/I6 read APIs, structured 404/409/503/400 errors, approval policy), I1 webhook listener (idempotent) |
| `crates/wb-mock` | Stand-in KG-Palantir research service serving the vendored real-run fixtures (`vendor/api_mock`) with live state transitions: WAITING_REVIEW → SUCCEEDED/CANCELLED, 409 conflicts, idempotent replay, 修改后通过 v2 writes into the vault |
| `crates/wb-app` | The GPUI desktop app (Windows): review inbox, brief detail with guardrail red-flagging, review modal with live diff, brief library with full-text search, event quadrant, audit trail with JSON export, service-status/degraded display |
| `crates/wb-e2e` | End-to-end suite: the full review loop over real HTTP (mock service + vault + webhook + guardrail + audit) |

## Run it

```bash
# 1. start the mock research service (terminal 1)
cd workbench
cargo run -p wb-mock -- --port 8012 --api-mock vendor/api_mock --vault research-vault

# 2. start the workbench (terminal 2)
cargo run -p wb-app                 # add --view library|quadrant|audit to start on a view
# config via wb.config.json or env: WB_API_BASE, WB_MCP_URL, WB_VAULT, WB_DATA,
# WB_WEBHOOK_PORT (default 8898), WB_REVIEWER

Keyboard: Ctrl+1–4 switch views · Ctrl+R refresh queue · Esc back / close review
form · Ctrl+Enter submit the review form
```

To exercise the I1 webhook path, start the mock with
`--webhook-url http://127.0.0.1:8898/webhook/i1` and trigger a notification:

```bash
curl -X POST http://127.0.0.1:8012/__test/emit_webhook -d '{"task_id":"RT-PR-EV-20261029-1225636931-S300806"}'
```

## Verify (tests + self-check)

```bash
cargo test                       # 13 unit tests (wb-core) + 5 e2e tests (wb-e2e)

# UI self-check: opens the real window, renders 4s, exits 0
cargo run -p wb-app -- --selfcheck

# UI end-to-end: drives the FULL review loop through the real UI state against a
# running research service — open task → guardrail flags 「1652万」 → review modal
# (修改后通过) → simulated edit → submit via MCP → asserts SUCCEEDED at brief v2,
# audit record written, queue updated, vault v2 clean.
WB_VAULT=research-vault WB_DATA=wb-data cargo run -p wb-app -- --selfcheck-e2e
# prints: SELFCHECK_E2E_OK
```

`screenshots/` holds captured evidence of the live app (inbox, review modal with
diff, post-review state with audit toast).

## The guardrail (F4) — the thesis discipline, in code

`wb_core::guardrail` extracts every number from the model-generated INFERENCE
section and requires it to match an evidence number (deterministic metrics +
announcement/excerpt numbers, 万/亿 scale-aware) within 1% relative (0.02pp
absolute). Model-computed *differences* are not evidence numbers:

- 斯迪克 2026Q3: “扣非低于归母约 **1652万**” (0.669378 − 0.504206 ≈ 0.1652亿) → flagged
- 永冠新材 2026Q3: “非经常性损益贡献约 **2460万**” (0.312934 − 0.066978 ≈ 0.246亿) → flagged
- the corrected rewrites quoting announcement numbers directly → pass

Violations render red-underlined in the brief detail with a tooltip; the review
modal's diff (F6) shows original vs revised while the v1 draft stays untouched
in the vault (`{task_id}.v1.md`) and the version bumps to v2.

## Known limitations

- Text inputs accept keystrokes + clipboard paste; IME composition (Chinese
  typing) is not wired up in this build — review comments can be pasted.
- gpui 0.2.2 layout quirks on Windows (grow items must be the last flex child;
  absolute insets are DPI-offset) are worked around in `workspace.rs`/`detail.rs`.
- The mock service is a stand-in: single-threaded, in-memory state, and its
  queue is generated from the loaded fixtures rather than the static
  `analysis_history` file for internal consistency.
