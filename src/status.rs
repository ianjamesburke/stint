/// Compute the `stint status` summary from a task graph.
use crate::schema::{extract_section, BlockedByRef, Task, TaskStatus};
use crate::state::{active_blockers_with_resolution, done_ids, BlockerResolution};

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

/// Summary of an open human decision gate (`tags` contains `gate`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSummary {
    /// Gate task ID.
    pub id: String,
    /// Gate title.
    pub title: String,
    /// Extracted `## Ask` section, if present.
    pub ask: Option<String>,
    /// IDs of open tasks blocked by this gate.
    pub blocks: Vec<String>,
}

/// Top-level status summary returned by [`compute_status`].
#[derive(Debug)]
pub struct StatusReport {
    /// Count of tasks with status backlog, todo, or in-progress (total open).
    pub open_count: usize,
    /// Count of tasks with status backlog (icebox only).
    pub backlog_count: usize,
    /// Count of tasks with status proposed (candidate ideas awaiting approval).
    pub proposed_count: usize,
    /// Count of tasks with status in-progress.
    pub in_progress_count: usize,
    /// Open decision gates awaiting a human answer.
    pub open_gates: Vec<GateSummary>,
    /// Todo+InProgress tasks that have at least one active blocker.
    pub blocked_tasks: Vec<BlockedTask>,
}

/// Compute the status summary.
pub fn compute_status(tasks: &[Task]) -> StatusReport {
    compute_status_with_resolution(tasks, &BlockerResolution::empty())
}

pub fn compute_status_with_resolution(
    tasks: &[Task],
    resolution: &BlockerResolution,
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

    let proposed_count = tasks
        .iter()
        .filter(|t| matches!(t.frontmatter.status, TaskStatus::Proposed))
        .count();

    let in_progress_count = tasks
        .iter()
        .filter(|t| matches!(t.frontmatter.status, TaskStatus::InProgress))
        .count();

    let open_gates = tasks
        .iter()
        .filter(|t| {
            t.is_gate()
                && !matches!(
                    t.frontmatter.status,
                    TaskStatus::Done | TaskStatus::Archived
                )
        })
        .map(|gate| {
            let gate_id = &gate.frontmatter.id;
            let blocks: Vec<String> = tasks
                .iter()
                .filter(|other| {
                    !matches!(
                        other.frontmatter.status,
                        TaskStatus::Done | TaskStatus::Archived
                    ) && other
                        .frontmatter
                        .blocked_by
                        .iter()
                        .any(|b| matches!(b, BlockedByRef::LocalTask(id) if id == gate_id))
                })
                .map(|other| other.frontmatter.id.clone())
                .collect();
            let ask = extract_section(&gate.body, "Ask").filter(|s| !s.is_empty());
            GateSummary {
                id: gate.frontmatter.id.clone(),
                title: gate.frontmatter.title.clone(),
                ask,
                blocks,
            }
        })
        .collect();

    let done = done_ids(tasks);

    let blocked_tasks = tasks
        .iter()
        .filter(|t| {
            // Backlog and proposed tasks are outside the active queue; only surface blockers for active tasks.
            matches!(
                t.frontmatter.status,
                TaskStatus::Todo | TaskStatus::InProgress
            )
        })
        .filter_map(|t| {
            let blocked_by = active_blockers_with_resolution(t, &done, resolution);
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

    StatusReport {
        open_count,
        backlog_count,
        proposed_count,
        in_progress_count,
        open_gates,
        blocked_tasks,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;

    fn make_task(
        id: &str,
        estimate: Option<&str>,
        actual: Option<&str>,
        status: TaskStatus,
    ) -> Task {
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

    #[test]
    fn open_count() {
        let tasks = vec![
            make_task("0001", None, None, TaskStatus::Backlog),
            make_task("0002", None, None, TaskStatus::Done),
            make_task("0003", None, None, TaskStatus::InProgress),
        ];
        let report = compute_status(&tasks);
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
        let report = compute_status(&[done, active]);
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
        let report = compute_status(&[blocker, task]);
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
        let report = compute_status(&[done, task]);
        assert!(report.blocked_tasks.is_empty());
    }

    #[test]
    fn backlog_count_reported_separately() {
        let tasks = vec![
            make_task("0001", None, None, TaskStatus::Backlog),
            make_task("0002", None, None, TaskStatus::Backlog),
            make_task("0003", None, None, TaskStatus::Todo),
            make_task("0004", None, None, TaskStatus::Done),
        ];
        let report = compute_status(&tasks);
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
        let report = compute_status(&[backlog_blocked, active_blocked]);
        assert_eq!(report.blocked_tasks.len(), 1);
        assert_eq!(report.blocked_tasks[0].id, "0002");
    }
}
