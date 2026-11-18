//! wb-core: logic for the WB financial-report review workbench.
//!
//! Pure Rust, no UI dependency — shared by the GPUI desktop app, the mock
//! research service and the e2e suite:
//! - `model`    — contract v3 shapes (tasks, briefs, context, evidence, reviews)
//! - `vault`    — I2 brief file contract (13 frontmatter fields + 4 sections) + search
//! - `guardrail`— F4 numeric guardrail (every number must trace to evidence)
//! - `provenance`— F4 layer 3: per-number citation to the source sentence (advisory)
//! - `presummary`— what changed since v1: sections, figures, new numbers (advisory)
//! - `structural`— F4 layer 2: accounting-identity re-computation over CALC values
//! - `analytics`— F13 review metrics + model-error taxonomy from the audit trail
//! - `llm`     — F14 local-LLM transport: SSE chat client for llama-server
//! - `research` — I3 MCP channel + I4/I6 read queries, structured errors, approval policy
//! - `webhook`  — I1 new-brief notifications (idempotent)
//! - `audit`    — F7 local-first review audit trail + JSON export
//! - `diff`     — F6 original-vs-revised line diff
//! - `duplicate`— same-event / restatement detection across briefs (advisory)
//! - `state`    — task state machine

pub mod audit;
pub mod analytics;
pub mod config;
pub mod diff;
pub mod duplicate;
pub mod guardrail;
pub mod llm;
pub mod model;
pub mod presummary;
pub mod provenance;
pub mod research;
pub mod state;
pub mod structural;
pub mod vault;
pub mod webhook;
