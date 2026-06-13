//! The single source of truth for "what state is a task in".
//!
//! Every consumer (`next`, `status`, `check`, future UIs) classifies tasks
//! through [`classify`] rather than inspecting `status` and blockers with
//! ad-hoc per-command filters. This keeps blocker semantics consistent across
//! the whole tool.

use std::collections::HashSet;

use crate::schema::{BlockedByRef, Task, TaskStatus};

/// Coarse lifecycle + blocker state of a task.
///
/// Derived from the task's `status` and its *active* blockers (blockers minus
/// any local task that is already done). This is intentionally a small, total
/// classification: every task maps to exactly one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Backlog. Iced; not in the actionable pool.
    Iced,
    /// Todo with no unresolved blockers. Claimable now.
    Ready,
    /// Todo with at least one unresolved blocker. Waiting.
    Blocked,
    /// In-progress. The current focus.
    Active,
    /// Done.
    Done,
    /// Archived.
    Archived,
}

impl TaskState {
    /// Lowercase display label for the state.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Iced => "backlog",
            TaskState::Ready => "ready",
            TaskState::Blocked => "blocked",
            TaskState::Active => "active",
            TaskState::Done => "done",
            TaskState::Archived => "archived",
        }
    }
}

/// Collect the IDs of all tasks whose status is `done`.
///
/// Returned as a set of borrows for cheap repeated blocker lookups.
pub fn done_ids(tasks: &[Task]) -> HashSet<&str> {
    tasks
        .iter()
        .filter(|task| matches!(task.frontmatter.status, TaskStatus::Done))
        .map(|task| task.frontmatter.id.as_str())
        .collect()
}

/// The active (unresolved) blockers for a task: its `blocked_by` list with any
/// local task that is already done filtered out. Non-task blockers (issues,
/// external refs, free-text notes) are always considered unresolved because
/// stint cannot verify them.
pub fn active_blockers(task: &Task, done_ids: &HashSet<&str>) -> Vec<BlockedByRef> {
    task.frontmatter
        .blocked_by
        .iter()
        .filter(|blocker| match blocker {
            BlockedByRef::LocalTask(id) => !done_ids.contains(id.as_str()),
            _ => true,
        })
        .cloned()
        .collect()
}

/// Whether a task has any active blocker.
pub fn is_blocked(task: &Task, done_ids: &HashSet<&str>) -> bool {
    !active_blockers(task, done_ids).is_empty()
}

/// Classify a task into its single canonical [`TaskState`].
///
/// `done_ids` must be the set produced by [`done_ids`] over the full task list.
pub fn classify(task: &Task, done_ids: &HashSet<&str>) -> TaskState {
    match task.frontmatter.status {
        TaskStatus::Done => TaskState::Done,
        TaskStatus::Archived => TaskState::Archived,
        TaskStatus::Backlog => TaskState::Iced,
        TaskStatus::InProgress => TaskState::Active,
        TaskStatus::Todo => {
            if is_blocked(task, done_ids) {
                TaskState::Blocked
            } else {
                TaskState::Ready
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;

    fn task(id: &str, status: &str, extra: &str) -> Task {
        let content =
            format!("---\nid: \"{id}\"\ntitle: \"T\"\nstatus: {status}\n{extra}\n---\n");
        parse_task(&content, &format!("{id}-t.md")).unwrap()
    }

    #[test]
    fn todo_without_blockers_is_ready() {
        let t = task("0001", "todo", "");
        assert_eq!(classify(&t, &HashSet::new()), TaskState::Ready);
    }

    #[test]
    fn todo_with_unresolved_local_blocker_is_blocked() {
        let t = task("0002", "todo", "blocked_by: [1]");
        assert_eq!(classify(&t, &HashSet::new()), TaskState::Blocked);
    }

    #[test]
    fn todo_with_done_blocker_is_ready() {
        let t = task("0002", "todo", "blocked_by: [1]");
        let done = HashSet::from(["0001"]);
        assert_eq!(classify(&t, &done), TaskState::Ready);
    }

    #[test]
    fn note_blocker_keeps_task_blocked() {
        let t = task("0003", "todo", "blocked_by: [\"waiting on design\"]");
        assert_eq!(classify(&t, &HashSet::new()), TaskState::Blocked);
    }

    #[test]
    fn backlog_is_iced_regardless_of_blockers() {
        let t = task("0004", "backlog", "blocked_by: [1]");
        assert_eq!(classify(&t, &HashSet::new()), TaskState::Iced);
    }

    #[test]
    fn in_progress_is_active_even_when_blocked() {
        // Illegal-but-present state: classifier reports lifecycle truth;
        // `check` is what flags this as an error.
        let t = task("0005", "in-progress", "blocked_by: [1]");
        assert_eq!(classify(&t, &HashSet::new()), TaskState::Active);
    }

    #[test]
    fn done_and_archived_map_directly() {
        assert_eq!(classify(&task("0006", "done", ""), &HashSet::new()), TaskState::Done);
        assert_eq!(
            classify(&task("0007", "archived", ""), &HashSet::new()),
            TaskState::Archived
        );
    }

    #[test]
    fn as_str_labels() {
        assert_eq!(TaskState::Iced.as_str(), "backlog");
        assert_eq!(TaskState::Ready.as_str(), "ready");
        assert_eq!(TaskState::Blocked.as_str(), "blocked");
        assert_eq!(TaskState::Active.as_str(), "active");
        assert_eq!(TaskState::Done.as_str(), "done");
        assert_eq!(TaskState::Archived.as_str(), "archived");
    }
}
