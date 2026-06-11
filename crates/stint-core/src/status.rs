/// Compute the `stint status` summary from a task graph.
use std::collections::HashSet;

use crate::gate::{active_blocker_infos, BlockerInfo};
use crate::schema::{Gate, Sprint, Task, TaskStatus};

/// A task that has at least one blocker set.
#[derive(Debug, Clone)]
pub struct BlockedTask {
    /// Task ID.
    pub id: String,
    /// Task title.
    pub title: String,
    /// All blocker references for this task.
    pub blocked_by: Vec<BlockerInfo>,
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
    /// Count of tasks with status backlog, todo, or in-progress.
    pub open_count: usize,
    /// Active tasks (non-done/archived) that have at least one blocker.
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
    gates: &[Gate],
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

    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|t| matches!(t.frontmatter.status, TaskStatus::Done))
        .map(|t| t.frontmatter.id.as_str())
        .collect();

    let blocked_tasks = tasks
        .iter()
        .filter(|t| {
            !matches!(
                t.frontmatter.status,
                TaskStatus::Done | TaskStatus::Archived
            )
        })
        .filter_map(|t| {
            let blocked_by = active_blocker_infos(t, gates, &done_ids);
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
        if !sprints.iter().any(|s| s.header.id == sid) {
            return None;
        }

        let sprint_tasks: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.frontmatter.sprint.as_deref() == Some(sid))
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
    use crate::parse::{parse_gate, parse_task};
    use crate::schema::{TaskFrontmatter, TaskStatus};
    use crate::sprint::parse_sprint;

    fn make_task_with_sprint(
        id: &str,
        sprint: &str,
        estimate: Option<&str>,
        actual: Option<&str>,
        status: TaskStatus,
    ) -> Task {
        use crate::schema::Task;
        Task {
            frontmatter: TaskFrontmatter {
                id: id.to_owned(),
                title: format!("Task {}", id),
                status,
                estimate: estimate.map(|s| s.parse().unwrap()),
                actual: actual.map(|s| s.parse().unwrap()),
                started_at: None,
                completed_at: None,
                sprint: Some(sprint.to_owned()),
                blocked_by: vec![],
                gh_issue: vec![],
                area: vec![],
                tags: vec![],
            },
            body: String::new(),
            filename: format!("{}-slug.md", id),
        }
    }

    fn make_blocked_task(id: &str) -> Task {
        let content = format!(
            "---\nid: \"{id}\"\ntitle: \"Blocked\"\nstatus: in-progress\nblocked_by:\n  - \"0000\"\n---\n"
        );
        parse_task(&content, &format!("{id}-slug.md")).unwrap()
    }

    fn make_sprint_file(number: u32) -> crate::schema::Sprint {
        let content = format!("# Sprint {number} · Jan 1–15 · goal: test\n\n- 0001\n");
        parse_sprint(&content).unwrap()
    }

    #[test]
    fn open_count() {
        let tasks = vec![
            make_task_with_sprint("0001", "s1", None, None, TaskStatus::Backlog),
            make_task_with_sprint("0002", "s1", None, None, TaskStatus::Done),
            make_task_with_sprint("0003", "s1", None, None, TaskStatus::InProgress),
        ];
        let report = compute_status(&tasks, &[], &[], None);
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
        let report = compute_status(&[done, active], &[], &[], None);
        assert_eq!(report.blocked_tasks.len(), 1);
        assert_eq!(report.blocked_tasks[0].id, "0002");
    }

    #[test]
    fn blocked_tasks_include_matching_gate_blockers() {
        let task = parse_task(
            "---\nid: \"0001\"\ntitle: \"V2\"\nstatus: todo\ntags: [\"v2\"]\n---\n",
            "0001-v2.md",
        )
        .unwrap();
        let gate = parse_gate(
            "---\nid: v2-after-v1\napplies_to:\n  tags: [\"v2\"]\nblocked_by:\n  - 0030\n---\n",
            "v2-after-v1.md",
        )
        .unwrap();
        let report = compute_status(&[task], &[], &[gate], None);
        assert_eq!(report.blocked_tasks.len(), 1);
        assert_eq!(
            report.blocked_tasks[0].blocked_by[0].reference.to_string(),
            "0030"
        );
    }

    #[test]
    fn blocked_tasks_ignore_done_gate_blockers() {
        let done = parse_task(
            "---\nid: \"0030\"\ntitle: \"Done\"\nstatus: done\n---\n",
            "0030-done.md",
        )
        .unwrap();
        let task = parse_task(
            "---\nid: \"0001\"\ntitle: \"V2\"\nstatus: todo\ntags: [\"v2\"]\n---\n",
            "0001-v2.md",
        )
        .unwrap();
        let gate = parse_gate(
            "---\nid: v2-after-v1\napplies_to:\n  tags: [\"v2\"]\nblocked_by:\n  - 0030\n---\n",
            "v2-after-v1.md",
        )
        .unwrap();
        let report = compute_status(&[done, task], &[], &[gate], None);
        assert!(report.blocked_tasks.is_empty());
    }

    #[test]
    fn sprint_progress_basic() {
        let sprint = make_sprint_file(1);
        let tasks = vec![
            make_task_with_sprint("0001", "s1", Some("4h"), Some("2h"), TaskStatus::InProgress),
            make_task_with_sprint("0002", "s1", Some("2h"), None, TaskStatus::Done),
        ];
        let report = compute_status(&tasks, &[sprint], &[], Some("s1"));
        let progress = report.sprint_progress.unwrap();
        assert_eq!(progress.committed_minutes, 360);
        assert_eq!(progress.logged_minutes, 120);
        assert_eq!(progress.remaining_minutes, 240);
        assert_eq!(progress.done_count, 1);
        assert_eq!(progress.task_count, 2);
    }

    #[test]
    fn no_sprints_returns_none_progress() {
        let report = compute_status(&[], &[], &[], None);
        assert!(report.sprint_progress.is_none());
    }

    #[test]
    fn latest_sprint_selected_when_unspecified() {
        let s1 = make_sprint_file(1);
        let s3 = make_sprint_file(3);
        let tasks = vec![make_task_with_sprint(
            "0001",
            "s3",
            Some("1h"),
            None,
            TaskStatus::Todo,
        )];
        let report = compute_status(&tasks, &[s1, s3], &[], None);
        let progress = report.sprint_progress.unwrap();
        assert_eq!(progress.sprint_id, "s3");
    }
}
