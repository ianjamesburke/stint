/// Compute the `stint status` summary from a task graph.
use std::collections::HashSet;

use crate::schema::{BlockedByRef, Sprint, Task, TaskStatus};
use crate::sprint::numeric_prefix;
use crate::state::{active_blockers, done_ids};

/// A task that has at least one blocker set.
#[derive(Debug, Clone)]
pub struct BlockedTask {
    /// Task ID.
    pub id: String,
    /// Task title.
    pub title: String,
    /// All active blocker references for this task.
    pub blocked_by: Vec<BlockedByRef>,
}

/// Sprint-level time progress.
#[derive(Debug, Clone)]
pub struct SprintProgress {
    /// Sprint identifier.
    pub sprint_id: String,
    /// Total estimated minutes across all tasks in this sprint.
    pub committed_minutes: u32,
    /// Total logged minutes across all tasks in this sprint.
    pub logged_minutes: u32,
    /// `committed_minutes - logged_minutes`, saturating at 0.
    pub remaining_minutes: u32,
    /// Number of tasks in this sprint.
    pub task_count: usize,
    /// Number of tasks with `status == done`.
    pub done_count: usize,
}

/// Top-level status summary returned by [`compute_status`].
#[derive(Debug)]
pub struct StatusReport {
    /// Count of tasks with status backlog, todo, or in-progress (total open).
    pub open_count: usize,
    /// Count of tasks with status backlog (icebox only).
    pub backlog_count: usize,
    /// Todo+InProgress tasks that have at least one active blocker.
    pub blocked_tasks: Vec<BlockedTask>,
    /// Progress for the current sprint, if one exists.
    pub sprint_progress: Option<SprintProgress>,
}

/// Compute the status summary.
///
/// `current_sprint` — if provided, use this sprint ID for the progress
/// section; otherwise the lexicographically last sprint ID is used.
pub fn compute_status(
    tasks: &[Task],
    sprints: &[Sprint],
    current_sprint: Option<&str>,
) -> StatusReport {
    let open_count = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.frontmatter.status,
                TaskStatus::Backlog | TaskStatus::Todo | TaskStatus::InProgress
            )
        })
        .count();

    let backlog_count = tasks
        .iter()
        .filter(|t| matches!(t.frontmatter.status, TaskStatus::Backlog))
        .count();

    let done = done_ids(tasks);

    let blocked_tasks = tasks
        .iter()
        .filter(|t| {
            // Backlog tasks are iced; only surface blockers for active tasks.
            matches!(
                t.frontmatter.status,
                TaskStatus::Todo | TaskStatus::InProgress
            )
        })
        .filter_map(|t| {
            let blocked_by = active_blockers(t, &done);
            if blocked_by.is_empty() {
                return None;
            }
            Some(BlockedTask {
                id: t.frontmatter.id.clone(),
                title: t.frontmatter.title.clone(),
                blocked_by,
            })
        })
        .collect();

    let sprint_id: Option<&str> = current_sprint.or_else(|| {
        sprints
            .iter()
            .map(|s| s.header.id.as_str())
            .max_by(|a, b| sprint_number(a).cmp(&sprint_number(b)))
    });

    let sprint_progress = sprint_id.and_then(|sid| {
        let sprint = sprints.iter().find(|s| s.header.id == sid)?;

        let sprint_task_ids: HashSet<&str> = sprint
            .task_ids
            .iter()
            .map(|e| numeric_prefix(e))
            .collect();

        let sprint_tasks: Vec<&Task> = tasks
            .iter()
            .filter(|t| sprint_task_ids.contains(t.frontmatter.id.as_str()))
            .collect();

        let committed_minutes: u32 = sprint_tasks
            .iter()
            .filter_map(|t| t.frontmatter.estimate)
            .map(|d| d.minutes())
            .sum();

        let logged_minutes: u32 = sprint_tasks
            .iter()
            .filter_map(|t| t.frontmatter.actual)
            .map(|d| d.minutes())
            .sum();

        let done_count = sprint_tasks
            .iter()
            .filter(|t| matches!(t.frontmatter.status, TaskStatus::Done))
            .count();

        let remaining_minutes = committed_minutes.saturating_sub(logged_minutes);

        Some(SprintProgress {
            sprint_id: sid.to_owned(),
            committed_minutes,
            logged_minutes,
            remaining_minutes,
            task_count: sprint_tasks.len(),
            done_count,
        })
    });

    StatusReport {
        open_count,
        backlog_count,
        blocked_tasks,
        sprint_progress,
    }
}

fn sprint_number(id: &str) -> u64 {
    id.trim_start_matches('s').parse().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;
    use crate::sprint::parse_sprint;

    fn make_task(id: &str, estimate: Option<&str>, actual: Option<&str>, status: TaskStatus) -> Task {
        let mut fields = String::new();
        if let Some(e) = estimate {
            fields.push_str(&format!("estimate: \"{e}\"\n"));
        }
        if let Some(a) = actual {
            fields.push_str(&format!("actual: \"{a}\"\n"));
        }
        let content = format!(
            "---\nid: \"{id}\"\ntitle: \"Task {id}\"\nstatus: {}\n{fields}---\n",
            status.as_str()
        );
        parse_task(&content, &format!("{id}-slug.md")).unwrap()
    }

    fn make_blocked_task(id: &str) -> Task {
        let content = format!(
            "---\nid: \"{id}\"\ntitle: \"Blocked\"\nstatus: in-progress\nblocked_by:\n  - \"0000\"\n---\n"
        );
        parse_task(&content, &format!("{id}-slug.md")).unwrap()
    }

    fn make_sprint_file(number: u32, task_ids: &[&str]) -> crate::schema::Sprint {
        let entries: String = task_ids.iter().map(|id| format!("- {id}\n")).collect();
        let content = format!("# Sprint {number} · Jan 1–15 · goal: test\n\n{entries}");
        parse_sprint(&content).unwrap()
    }

    #[test]
    fn open_count() {
        let tasks = vec![
            make_task("0001", None, None, TaskStatus::Backlog),
            make_task("0002", None, None, TaskStatus::Done),
            make_task("0003", None, None, TaskStatus::InProgress),
        ];
        let report = compute_status(&tasks, &[], None);
        assert_eq!(report.open_count, 2);
    }

    #[test]
    fn blocked_tasks_excludes_done() {
        let done = {
            let mut t = make_blocked_task("0001");
            t.frontmatter.status = TaskStatus::Done;
            t
        };
        let active = make_blocked_task("0002");
        let report = compute_status(&[done, active], &[], None);
        assert_eq!(report.blocked_tasks.len(), 1);
        assert_eq!(report.blocked_tasks[0].id, "0002");
    }

    #[test]
    fn blocked_tasks_report_active_local_blockers() {
        let blocker = parse_task(
            "---\nid: \"0030\"\ntitle: \"Blocker\"\nstatus: todo\n---\n",
            "0030-blocker.md",
        )
        .unwrap();
        let task = parse_task(
            "---\nid: \"0001\"\ntitle: \"Dep\"\nstatus: todo\nblocked_by:\n  - 0030\n---\n",
            "0001-dep.md",
        )
        .unwrap();
        let report = compute_status(&[blocker, task], &[], None);
        assert_eq!(report.blocked_tasks.len(), 1);
        assert_eq!(report.blocked_tasks[0].blocked_by[0].to_string(), "0030");
    }

    #[test]
    fn blocked_tasks_ignore_done_local_blockers() {
        let done = parse_task(
            "---\nid: \"0030\"\ntitle: \"Done\"\nstatus: done\n---\n",
            "0030-done.md",
        )
        .unwrap();
        let task = parse_task(
            "---\nid: \"0001\"\ntitle: \"Dep\"\nstatus: todo\nblocked_by:\n  - 0030\n---\n",
            "0001-dep.md",
        )
        .unwrap();
        let report = compute_status(&[done, task], &[], None);
        assert!(report.blocked_tasks.is_empty());
    }

    #[test]
    fn sprint_progress_basic() {
        // Sprint index lists 0001 and 0002.
        let sprint = make_sprint_file(1, &["0001", "0002"]);
        let tasks = vec![
            make_task("0001", Some("4h"), Some("2h"), TaskStatus::InProgress),
            make_task("0002", Some("2h"), None, TaskStatus::Done),
        ];
        let report = compute_status(&tasks, &[sprint], Some("s1"));
        let progress = report.sprint_progress.unwrap();
        assert_eq!(progress.committed_minutes, 360);
        assert_eq!(progress.logged_minutes, 120);
        assert_eq!(progress.remaining_minutes, 240);
        assert_eq!(progress.done_count, 1);
        assert_eq!(progress.task_count, 2);
    }

    #[test]
    fn no_sprints_returns_none_progress() {
        let report = compute_status(&[], &[], None);
        assert!(report.sprint_progress.is_none());
    }

    #[test]
    fn latest_sprint_selected_when_unspecified() {
        // s3 is the latest; it lists 0001.
        let s1 = make_sprint_file(1, &[]);
        let s3 = make_sprint_file(3, &["0001"]);
        let tasks = vec![make_task("0001", Some("1h"), None, TaskStatus::Todo)];
        let report = compute_status(&tasks, &[s1, s3], None);
        let progress = report.sprint_progress.unwrap();
        assert_eq!(progress.sprint_id, "s3");
    }

    #[test]
    fn backlog_count_reported_separately() {
        let tasks = vec![
            make_task("0001", None, None, TaskStatus::Backlog),
            make_task("0002", None, None, TaskStatus::Backlog),
            make_task("0003", None, None, TaskStatus::Todo),
            make_task("0004", None, None, TaskStatus::Done),
        ];
        let report = compute_status(&tasks, &[], None);
        assert_eq!(report.backlog_count, 2);
        assert_eq!(report.open_count, 3); // Backlog+Todo still open
    }

    #[test]
    fn blocked_tasks_excludes_backlog() {
        let backlog_blocked = parse_task(
            "---\nid: \"0001\"\ntitle: \"Iced\"\nstatus: backlog\nblocked_by:\n  - \"0000\"\n---\n",
            "0001-slug.md",
        )
        .unwrap();
        let active_blocked = make_blocked_task("0002");
        let report = compute_status(&[backlog_blocked, active_blocked], &[], None);
        assert_eq!(report.blocked_tasks.len(), 1);
        assert_eq!(report.blocked_tasks[0].id, "0002");
    }

    #[test]
    fn sprint_progress_only_counts_tasks_in_index() {
        // Task 0002 is NOT in the sprint index — should not be counted.
        let sprint = make_sprint_file(1, &["0001"]);
        let tasks = vec![
            make_task("0001", Some("1h"), None, TaskStatus::Todo),
            make_task("0002", Some("2h"), None, TaskStatus::Todo),
        ];
        let report = compute_status(&tasks, &[sprint], Some("s1"));
        let progress = report.sprint_progress.unwrap();
        assert_eq!(progress.task_count, 1);
        assert_eq!(progress.committed_minutes, 60);
    }
}
