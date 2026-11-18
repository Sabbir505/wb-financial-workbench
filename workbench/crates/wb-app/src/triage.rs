//! Triage: order the review inbox by importance × urgency (spec §2.3) or by
//! arrival (FIFO), and explain the ordering to the reviewer.
//!
//! ## Where the priority comes from
//!
//! Not from the wire. `TaskSummary.priority` is whatever the research service
//! asserted, and the service's own `sections.check` self-report is provably
//! unreliable in this repo (the 斯迪克 fixture records 校验通过 while its INFERENCE
//! carries an untraceable 1652万). So the workbench computes priority from the
//! deterministic guardrail verdict, which means it must run the guardrail for
//! each queued brief — cheap (no network, no LLM) and, more importantly, the same
//! verdict the detail view will show.
//!
//! ## Why the ordering lives here and not on the server
//!
//! §2.4b compares quadrant ordering against FIFO. That comparison is only
//! meaningful if both orderings are applied to the same queue by the same code,
//! with the mode as the only difference. Sorting server-side would make the
//! experiment a comparison of two services.

use std::collections::BTreeMap;

use wb_core::guardrail::{build_evidence_numbers, check, EvidenceInputs, Guardrail};
use wb_core::model::TaskSummary;
use wb_core::state::{order_queue, priority_for, OrderKey, Priority, SortMode};
use wb_core::vault::{SectionKind, Vault};

/// A queue row plus the verdict the workbench computed for it.
#[derive(Clone, Debug)]
pub struct TriagedRow {
    pub task: TaskSummary,
    pub priority: Priority,
    /// True when the brief's inference carries at least one untraceable number.
    pub flagged: bool,
    /// How many numbers could not be traced. Zero when `flagged` is false.
    pub violation_count: usize,
}

impl TriagedRow {
    /// Why this row sits where it does — the "explain the ordering" half of the
    /// feature. Localised by the caller; the sentence shape lives here.
    pub fn reason(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(if self.flagged {
            format!("{} untraceable number(s)", self.violation_count)
        } else {
            "all numbers traceable".to_string()
        });
        parts.push(if self.priority.urgent {
            "waiting 24h+".into()
        } else {
            "recent".into()
        });
        format!("{} — {}", self.priority.code(), parts.join(", "))
    }
}

/// Run the guardrail for one queued brief using the vault (offline path).
///
/// Returns `None` when there is no brief to check — a task in WAITING_DATA or
/// IGNORED has no inference section, so it has nothing to flag and is not
/// "clean"; it is simply not applicable. Treating it as clean would inflate the
/// false-negative count in the §2.4a experiment.
pub fn guardrail_for_brief(vault: &Vault, task_id: &str) -> Option<Guardrail> {
    let brief = vault.read_brief(task_id)?;
    let inference = brief.section(SectionKind::Inference)?.body.clone();
    if inference.trim().is_empty() {
        return None;
    }
    let mut inputs = EvidenceInputs::default();
    inputs
        .metrics
        .extend(brief.calc_values().values().copied());
    if let Some(fact) = brief.section(SectionKind::Fact) {
        inputs.free_texts.push(fact.body.clone());
    }
    Some(check(
        &inference,
        &build_evidence_numbers(&inputs),
        &Default::default(),
    ))
}

/// Compute priority for every row, then order them.
pub fn triage(
    queue: &[TaskSummary],
    vault: &Vault,
    mode: SortMode,
    now: &chrono::NaiveDateTime,
) -> Vec<TriagedRow> {
    let mut rows: Vec<TriagedRow> = queue
        .iter()
        .map(|t| {
            let g = guardrail_for_brief(vault, &t.task_id);
            let flagged = g.as_ref().map(|g| !g.ok()).unwrap_or(false);
            TriagedRow {
                task: t.clone(),
                priority: priority_for(flagged, &t.updated_at, now),
                flagged,
                violation_count: g.map(|g| g.violations.len()).unwrap_or(0),
            }
        })
        .collect();

    let keys: Vec<OrderKey> = rows
        .iter()
        .map(|r| OrderKey {
            task_id: r.task.task_id.clone(),
            priority: r.priority,
            created_at: r.task.created_at.clone(),
        })
        .collect();
    let order = order_queue(&keys, mode);

    let mut by_id: BTreeMap<String, TriagedRow> = BTreeMap::new();
    for r in rows.drain(..) {
        by_id.insert(r.task.task_id.clone(), r);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use wb_core::model::TaskSummary;

    const SDIC: &str = "RT-PR-EV-20261029-1225636931-S300806";

    fn repo_vault() -> Vault {
        // The repo's own fixtures: real vault files, no synthetic evidence.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("research-vault");
        Vault::new(root)
    }

    fn summary(task: &str, created: &str, updated: &str) -> TaskSummary {
        TaskSummary {
            task_id: task.into(),
            created_at: created.into(),
            updated_at: updated.into(),
            ..Default::default()
        }
    }

    fn now() -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str("2026-11-18T10:00:00", "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    #[test]
    fn missing_brief_is_not_applicable_rather_than_clean() {
        let v = repo_vault();
        assert!(guardrail_for_brief(&v, "RT-does-not-exist").is_none());
    }

    #[test]
    fn a_task_without_a_brief_is_not_counted_as_clean() {
        let v = repo_vault();
        let q = vec![summary("RT-missing", "2026-11-01T00:00:00", "2026-11-01T00:00:00")];
        let rows = triage(&q, &v, SortMode::Quadrant, &now());
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].flagged, "nothing to flag");
        assert_eq!(
            rows[0].violation_count, 0,
            "not applicable must not read as 'checked and clean'"
        );
    }

    #[test]
    fn quadrant_mode_orders_by_priority_then_arrival() {
        let v = repo_vault();
        let q = vec![
            summary("RT-b", "2026-11-10T00:00:00", "2026-11-17T00:00:00"),
            summary("RT-a", "2026-11-01T00:00:00", "2026-11-01T00:00:00"),
        ];
        let rows = triage(&q, &v, SortMode::Quadrant, &now());
        // Neither has a brief, so neither is flagged; both are stale, so both
        // are P3 and the tie breaks by arrival.
        assert_eq!(rows[0].task.task_id, "RT-a");
        assert_eq!(rows[1].task.task_id, "RT-b");
    }

    #[test]
    fn fifo_mode_orders_purely_by_arrival() {
        let v = repo_vault();
        let q = vec![
            summary("RT-late", "2026-11-10T00:00:00", "2026-11-17T00:00:00"),
            summary("RT-early", "2026-11-01T00:00:00", "2026-11-17T00:00:00"),
        ];
        let rows = triage(&q, &v, SortMode::Fifo, &now());
        assert_eq!(rows[0].task.task_id, "RT-early");
    }

    #[test]
    fn the_two_modes_can_disagree_which_is_the_point_of_the_experiment() {
        let v = repo_vault();
        let q = vec![
            // Arrives first, but touched an hour ago (P4: clean + fresh).
            summary("RT-old-clean", "2026-11-01T00:00:00", "2026-11-18T09:00:00"),
            // Arrives later, but untouched since 1 Nov (P3: clean + stale).
            summary("RT-new-stale", "2026-11-10T00:00:00", "2026-11-01T00:00:00"),
        ];
        let quad = triage(&q, &v, SortMode::Quadrant, &now());
        let fifo = triage(&q, &v, SortMode::Fifo, &now());
        assert_eq!(quad[0].task.task_id, "RT-new-stale");
        assert_eq!(fifo[0].task.task_id, "RT-old-clean");
    }

    #[test]
    fn reason_explains_the_placement() {
        let v = repo_vault();
        let q = vec![summary("RT-x", "2026-11-01T00:00:00", "2026-11-01T00:00:00")];
        let rows = triage(&q, &v, SortMode::Quadrant, &now());
        let r = rows[0].reason();
        assert!(r.contains("P3"), "got {r}");
        assert!(r.contains("waiting 24h+"), "got {r}");
    }

    /// The headline case, through the real vault: 斯迪克 v2 has no violation, but
    /// the preserved `.v1.md` does. This is why triage must read the brief that
    /// is actually pending, not "the brief for this task".
    #[test]
    fn the_pending_brief_is_the_one_checked_not_any_version() {
        let v = repo_vault();
        let g = guardrail_for_brief(&v, SDIC).expect("v2 brief exists");
        assert!(
            g.ok(),
            "v2 is the reviewed, corrected version; violations: {:?}",
            g.violation_tokens()
        );
    }
}
