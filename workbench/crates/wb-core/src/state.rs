//! Task state machine (contract v3 §8).
//! RECEIVED → TASK_CREATED → CONTEXT_READY → RUNNING → WAITING_REVIEW → SUCCEEDED
//! exception states: IGNORED / WAITING_DATA / FAILED / CANCELLED

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Received,
    TaskCreated,
    ContextReady,
    Running,
    WaitingReview,
    Succeeded,
    Ignored,
    WaitingData,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn parse(s: &str) -> Option<TaskState> {
        Some(match s {
            "RECEIVED" => TaskState::Received,
            "TASK_CREATED" => TaskState::TaskCreated,
            "CONTEXT_READY" => TaskState::ContextReady,
            "RUNNING" => TaskState::Running,
            "WAITING_REVIEW" => TaskState::WaitingReview,
            "SUCCEEDED" => TaskState::Succeeded,
            "IGNORED" => TaskState::Ignored,
            "WAITING_DATA" => TaskState::WaitingData,
            "FAILED" => TaskState::Failed,
            "CANCELLED" => TaskState::Cancelled,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Received => "RECEIVED",
            TaskState::TaskCreated => "TASK_CREATED",
            TaskState::ContextReady => "CONTEXT_READY",
            TaskState::Running => "RUNNING",
            TaskState::WaitingReview => "WAITING_REVIEW",
            TaskState::Succeeded => "SUCCEEDED",
            TaskState::Ignored => "IGNORED",
            TaskState::WaitingData => "WAITING_DATA",
            TaskState::Failed => "FAILED",
            TaskState::Cancelled => "CANCELLED",
        }
    }

    /// The review inbox only acts on WAITING_REVIEW.
    pub fn is_reviewable(&self) -> bool {
        *self == TaskState::WaitingReview
    }

    pub fn is_exception(&self) -> bool {
        matches!(
            self,
            TaskState::Ignored | TaskState::WaitingData | TaskState::Failed | TaskState::Cancelled
        )
    }

    /// Review outcomes per the mock samples: 通过→SUCCEEDED, 驳回→CANCELLED,
    /// 修改后通过→SUCCEEDED with a brief version bump.
    pub fn after_review(verdict: crate::model::Verdict) -> TaskState {
        match verdict {
            crate::model::Verdict::Reject => TaskState::Cancelled,
            _ => TaskState::Succeeded,
        }
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Eisenhower placement for the queue view (F12): importance from priority
/// (P1/P2 = important), urgency from recency (updated within 24h). Returns the
/// quadrant index 0..=3 (P1 urgent×important … P4 neither).
pub fn quadrant_of(priority: &str, updated_at: &str, now: &chrono::NaiveDateTime) -> usize {
    let important = matches!(priority, "P1" | "P2");
    let urgent = chrono::NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%dT%H:%M:%S")
        .map(|t| (*now - t).num_hours() < 24)
        .unwrap_or(false);
    match (urgent, important) {
        (true, true) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (false, false) => 3,
    }
}

/// Deterministic task priority (supervisor spec §2.3): importance × urgency.
///
/// Both axes come from task fields alone — no fixture-supplied label, no wall
/// clock beyond the caller's `now` — so the same task always yields the same
/// code. That property is what makes the quadrant-vs-FIFO experiment (§2.4b)
/// measurable: the ordering must be a function of the data, not of the JSON.
///
/// - **importance**: the brief carries a number that could not be traced to
///   evidence. `sections.check` is the research service's recorded guardrail
///   verdict (`数字校验未通过：['0.246']` vs `数字校验通过`). An untraceable
///   figure is the one defect this desk exists to stop, so a flagged brief
///   outranks a clean one regardless of age.
/// - **urgency**: the task has waited at least 24h since its last transition.
///   Age, not recency — a brief that has sat for three days is more urgent than
///   one that arrived an hour ago.
///
/// NOTE: [`quadrant_of`] measures urgency as *recency* (updated within 24h),
/// which is the opposite convention. The two disagree on identical input; see
/// `aging_and_recency_disagree` below. `quadrant_of` is retained only for the
/// wire-level P-string path, not as the priority rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priority {
    pub important: bool,
    pub urgent: bool,
}

impl Priority {
    /// P1 urgent×important · P2 important only · P3 urgent only · P4 neither.
    pub fn code(&self) -> &'static str {
        match (self.urgent, self.important) {
            (true, true) => "P1",
            (false, true) => "P2",
            (true, false) => "P3",
            (false, false) => "P4",
        }
    }

    /// Quadrant index 0..=3, same convention as [`quadrant_of`].
    pub fn quadrant(&self) -> usize {
        match (self.urgent, self.important) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
        }
    }
}

/// The rule itself. `flagged` must come from the deterministic guardrail, NOT
/// from `sections.check`: the 斯迪克 fixture records `数字校验通过` while its
/// INFERENCE carries the 1652万 violation, so the service's self-report is not
/// a trustworthy signal (see `check_line_contradicts_guardrail` below).
pub fn priority_for(
    flagged: bool,
    updated_at: &str,
    now: &chrono::NaiveDateTime,
) -> Priority {
    let important = flagged;
    let urgent = chrono::NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%dT%H:%M:%S")
        .map(|t| (*now - t).num_hours() >= 24)
        .unwrap_or(false);
    Priority { important, urgent }
}

/// Inbox ordering: the quadrant rule, or plain arrival order as the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    /// Importance × urgency (spec §2.3), then oldest-first within a quadrant.
    Quadrant,
    /// Arrival order — the control condition for the §2.4b comparison.
    Fifo,
}

impl SortMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SortMode::Quadrant => "quadrant",
            SortMode::Fifo => "fifo",
        }
    }

    pub fn parse(s: &str) -> Option<SortMode> {
        match s {
            "quadrant" => Some(SortMode::Quadrant),
            "fifo" => Some(SortMode::Fifo),
            _ => None,
        }
    }

    pub fn toggle(self) -> SortMode {
        match self {
            SortMode::Quadrant => SortMode::Fifo,
            SortMode::Fifo => SortMode::Quadrant,
        }
    }
}

/// One row's ordering key. Kept separate from `TaskSummary` so the ordering can
/// be tested without constructing a whole wire payload.
#[derive(Clone, Debug)]
pub struct OrderKey {
    pub task_id: String,
    pub priority: Priority,
    pub created_at: String,
}

/// Order a queue. `Quadrant` puts P1 first, then P2/P3/P4, breaking ties by
/// arrival (oldest first) so the rule is total — an unstable sort would make
/// the §2.4b measurement depend on sort implementation rather than on policy.
///
/// `Fifo` ignores priority entirely and orders by `created_at`, which is the
/// control the experiment compares against.
pub fn order_queue(keys: &[OrderKey], mode: SortMode) -> Vec<String> {
    let mut out: Vec<&OrderKey> = keys.iter().collect();
    out.sort_by(|a, b| match mode {
        SortMode::Fifo => a.created_at.cmp(&b.created_at).then_with(|| a.task_id.cmp(&b.task_id)),
        SortMode::Quadrant => a
            .priority
            .code()
            .cmp(b.priority.code())
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.task_id.cmp(&b.task_id)),
    });
    out.into_iter().map(|k| k.task_id.clone()).collect()
}

/// Convenience wrapper for callers holding a whole task. `flagged` is passed in
/// because only the caller knows the evidence set the guardrail ran against.
pub fn priority_of_task(
    detail: &crate::model::TaskDetail,
    flagged: bool,
    now: &chrono::NaiveDateTime,
) -> Priority {
    priority_for(flagged, &detail.updated_at, now)
}

/// Kept deliberately as documentation of a trap, not as a signal source.
///
/// Reading priority off `sections.check` looks reasonable and is wrong: the
/// 斯迪克 fixture's CHECK line says 校验通过 while its INFERENCE carries the
/// untraceable 1652万. The service's self-report and the deterministic guardrail
/// disagree, and the guardrail is authoritative. Only used by tests.
#[allow(dead_code)]
fn brief_has_untraceable_number(check: &str) -> bool {
    check.contains("未通过") || check.contains("无法追溯")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Verdict;

    fn detail_at(check: &str, updated_at: &str) -> crate::model::TaskDetail {
        crate::model::TaskDetail {
            task_id: "RT-T".into(),
            status: "WAITING_REVIEW".into(),
            updated_at: updated_at.into(),
            sections: crate::model::BriefSections {
                check: check.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// The real 斯迪克 fixture: CHECK says passed, INFERENCE carries the
    /// untraceable 1652万. Priority must follow the guardrail, not the CHECK line.
    #[test]
    fn check_line_contradicts_guardrail() {
        let d = detail_at("数字校验通过：推断节数字全部命中确定性计算或公告摘录", "2026-11-02T07:17:03");
        let from_check = brief_has_untraceable_number(&d.sections.check);
        assert!(!from_check, "the fixture's CHECK line claims a pass");
        // Flagged by the guardrail → important, even though CHECK says clean.
        let p = priority_of_task(&d, true, &now());
        assert!(p.important);
        assert_eq!(p.code(), "P2"); // ~5h old → important but not yet urgent
    }

    fn now() -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str("2026-11-02T12:00:00", "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    #[test]
    fn priority_covers_all_four_quadrants() {
        let fresh = "2026-11-02T10:00:00";
        let old = "2026-10-28T10:00:00";
        assert_eq!(priority_for(true, fresh, &now()).code(), "P2");
        assert_eq!(priority_for(false, fresh, &now()).code(), "P4");
        assert_eq!(priority_for(true, old, &now()).code(), "P1");
        assert_eq!(priority_for(false, old, &now()).code(), "P3");
    }

    #[test]
    fn priority_is_deterministic() {
        let old = "2026-10-28T10:00:00";
        // Same input, same output — the property §2.4b depends on.
        assert_eq!(priority_for(true, old, &now()), priority_for(true, old, &now()));
        // Priority is a pure function of (flagged, age); no wire label involved.
        assert_eq!(priority_for(false, old, &now()).code(), "P3");
    }

    #[test]
    fn flagged_is_important_regardless_of_age() {
        assert!(priority_for(true, "2026-01-01T00:00:00", &now()).important);
        assert!(!priority_for(false, "2026-11-02T10:00:00", &now()).important);
    }

    /// Documents the convention clash between the two urgency definitions.
    #[test]
    fn aging_and_recency_disagree() {
        // Fresh task: urgent under quadrant_of (recency), not under priority_for (aging).
        let fresh = "2026-11-02T10:00:00";
        let p = priority_for(false, fresh, &now());
        assert!(!p.urgent, "aging: 2h old is not urgent");
        assert_eq!(quadrant_of("P4", fresh, &now()), 2, "recency: 2h old is urgent");
    }

    #[test]
    fn parses_all_states() {
        for s in [
            "RECEIVED",
            "TASK_CREATED",
            "CONTEXT_READY",
            "RUNNING",
            "WAITING_REVIEW",
            "SUCCEEDED",
            "IGNORED",
            "WAITING_DATA",
            "FAILED",
            "CANCELLED",
        ] {
            assert_eq!(TaskState::parse(s).unwrap().as_str(), s);
        }
        assert!(TaskState::parse("NOPE").is_none());
    }

    #[test]
    fn review_outcomes() {
        assert!(TaskState::WaitingReview.is_reviewable());
        assert!(!TaskState::Failed.is_reviewable());
        assert_eq!(
            TaskState::after_review(Verdict::Approve),
            TaskState::Succeeded
        );
        assert_eq!(
            TaskState::after_review(Verdict::ApproveAsRevised),
            TaskState::Succeeded
        );
        assert_eq!(
            TaskState::after_review(Verdict::Reject),
            TaskState::Cancelled
        );
    }

    fn key(task: &str, flagged: bool, updated: &str, created: &str) -> OrderKey {
        OrderKey {
            task_id: task.into(),
            priority: priority_for(flagged, updated, &now()),
            created_at: created.into(),
        }
    }

    #[test]
    fn quadrant_order_puts_flagged_and_stale_first() {
        // P1 flagged+stale, P2 flagged+fresh, P3 clean+stale, P4 clean+fresh.
        let keys = vec![
            key("p4", false, "2026-11-02T10:00:00", "2026-11-01T00:00:00"),
            key("p1", true, "2026-10-20T10:00:00", "2026-11-02T00:00:00"),
            key("p3", false, "2026-10-20T10:00:00", "2026-11-03T00:00:00"),
            key("p2", true, "2026-11-02T10:00:00", "2026-11-04T00:00:00"),
        ];
        let ordered = order_queue(&keys, SortMode::Quadrant);
        assert_eq!(ordered, vec!["p1", "p2", "p3", "p4"]);
    }

    #[test]
    fn fifo_order_ignores_priority_entirely() {
        let keys = vec![
            key("p1", true, "2026-10-20T10:00:00", "2026-11-04T00:00:00"),
            key("p4", false, "2026-11-02T10:00:00", "2026-11-01T00:00:00"),
        ];
        let ordered = order_queue(&keys, SortMode::Fifo);
        assert_eq!(ordered, vec!["p4", "p1"], "arrival order, not priority");
    }

    #[test]
    fn ties_break_by_arrival_so_the_order_is_total() {
        let keys = vec![
            key("late", true, "2026-10-20T10:00:00", "2026-11-05T00:00:00"),
            key("early", true, "2026-10-20T10:00:00", "2026-11-01T00:00:00"),
        ];
        assert_eq!(order_queue(&keys, SortMode::Quadrant), vec!["early", "late"]);
    }

    #[test]
    fn ordering_is_deterministic_regardless_of_input_order() {
        let mut keys = vec![
            key("a", true, "2026-10-20T10:00:00", "2026-11-01T00:00:00"),
            key("b", false, "2026-11-02T10:00:00", "2026-11-02T00:00:00"),
            key("c", true, "2026-11-02T10:00:00", "2026-11-03T00:00:00"),
        ];
        let first = order_queue(&keys, SortMode::Quadrant);
        keys.reverse();
        let second = order_queue(&keys, SortMode::Quadrant);
        assert_eq!(first, second, "the rule must not depend on input order");
    }

    #[test]
    fn sort_mode_roundtrips_through_its_string() {
        for m in [SortMode::Quadrant, SortMode::Fifo] {
            assert_eq!(SortMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(SortMode::Quadrant.toggle(), SortMode::Fifo);
        assert_eq!(SortMode::parse("bogus"), None);
    }

    #[test]
    fn quadrant_placement() {
        let now = chrono::NaiveDateTime::parse_from_str("2026-11-02T12:00:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap();
        assert_eq!(
            quadrant_of("P1", "2026-11-02T07:17:03", &now),
            0,
            "urgent+important"
        );
        assert_eq!(
            quadrant_of("P2", "2026-10-01T07:17:03", &now),
            1,
            "important, stale"
        );
        assert_eq!(
            quadrant_of("P3", "2026-11-02T07:17:03", &now),
            2,
            "urgent, minor"
        );
        assert_eq!(quadrant_of("P4", "2026-10-01T07:17:03", &now), 3, "neither");
        assert_eq!(
            quadrant_of("P1", "not-a-date", &now),
            1,
            "unparseable → not urgent"
        );
    }
}
