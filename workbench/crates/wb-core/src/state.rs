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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Verdict;

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
