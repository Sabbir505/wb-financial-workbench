//! wb-core: logic for the WB financial-report review workbench.
//!
//! Pure Rust, no UI dependency — shared by the GPUI desktop app, the mock
//! research service and the e2e suite:
//! - `model`    — contract v3 shapes (tasks, briefs, context, evidence, reviews)
//! - `vault`    — I2 brief file contract (13 frontmatter fields + 4 sections) + search
//! - `guardrail`— F4 numeric guardrail (every number must trace to evidence)
//! - `research` — I3 MCP channel + I4/I6 read queries, structured errors, approval policy
//! - `webhook`  — I1 new-brief notifications (idempotent)
//! - `audit`    — F7 local-first review audit trail + JSON export
//! - `diff`     — F6 original-vs-revised line diff
//! - `state`    — task state machine

pub mod audit;
pub mod config;
pub mod diff;
pub mod guardrail;
pub mod model;
pub mod research;
pub mod state;
pub mod vault;
pub mod webhook;
