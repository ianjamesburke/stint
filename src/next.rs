use std::collections::{HashMap, HashSet};

use crate::schema::{cmp_priority, BlockedByRef, Priority, Sprint, Task, TaskStatus};
use crate::sprint::numeric_prefix;
use crate::state::{active_blockers, done_ids};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextOptions<'a> {
    pub sprint: Option<&'a str>,
    pub include_area_conflicts: bool,
    /// When true, backlog tasks are evaluated alongside todo tasks (appear in ready/blocked per
    /// their blockers). Default false: backlog tasks are completely excluded (icebox).
    pub include_backlog: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextTask {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub priority: Option<Priority>,
    pub sprint: Option<String>,
    pub area: Vec<String>,
    pub gh_issue: Vec<String>,
    pub filename: String,
    pub blockers: Vec<BlockedByRef>,
    /// IDs of in-progress tasks whose area this task shares (resource busy).
    pub area_conflicts: Vec<String>,
    /// IDs of earlier ready tasks selected this run whose area this task shares
    /// (would collide if both started together).
    pub selected_conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextReport {
    pub ready: Vec<NextTask>,
    pub blocked: Vec<NextTask>,
}

pub fn compute_next(tasks: &[Task], sprints: &[Sprint], options: NextOptions<'_>) -> NextReport {
    let done = done_ids(tasks);
    let active_area_tasks = active_area_tasks(tasks);
    let sprint_task_ids = sprint_task_ids(sprints, options.sprint);
    let mut ordered = ordered_tasks(tasks, sprints, options.sprint);
    ordered.sort_by(|a, b| compare_next_order(a, b));

    // Build task_id -> sprint_id lookup from sprint index files.
    let task_sprint: HashMap<&str, &str> = sprints
        .iter()
        .flat_map(|s| {
            s.task_ids
                .iter()
                .map(move |e| (numeric_prefix(e), s.header.id.as_str()))
        })
        .collect();

    let mut ready = Vec::new();
    let mut blocked = Vec::new();
    // area name -> id of the ready task that first claimed it this run.
    let mut claimed_areas: HashMap<String, String> = HashMap::new();

    for task in ordered {
        if !is_candidate(task, sprint_task_ids.as_ref(), options.include_backlog) {
            continue;
        }

        let blockers = active_blockers(task, &done);
        let mut area_conflicts = area_conflicts(task, &active_area_tasks);
        area_conflicts.sort();
        area_conflicts.dedup();

        let mut selected_conflicts: Vec<String> = task
            .frontmatter
            .area
            .iter()
            .filter_map(|area| claimed_areas.get(area).cloned())
            .collect();
        selected_conflicts.sort();
        selected_conflicts.dedup();

        let row = NextTask {
            id: task.frontmatter.id.clone(),
            title: task.frontmatter.title.clone(),
            status: task.frontmatter.status.clone(),
            priority: task.frontmatter.priority,
            sprint: task_sprint
                .get(task.frontmatter.id.as_str())
                .map(|s| s.to_string()),
            area: task.frontmatter.area.clone(),
            gh_issue: task.frontmatter.gh_issue.clone(),
            filename: task.filename.clone(),
            blockers: blockers.clone(),
            area_conflicts: area_conflicts.clone(),
            selected_conflicts: selected_conflicts.clone(),
        };

        if !blockers.is_empty() {
            blocked.push(row);
            continue;
        }

        if !options.include_area_conflicts
            && (!area_conflicts.is_empty() || !selected_conflicts.is_empty())
        {
            blocked.push(row);
            continue;
        }

        for area in &task.frontmatter.area {
            claimed_areas
                .entry(area.clone())
                .or_insert_with(|| task.frontmatter.id.clone());
        }
        ready.push(row);
    }

    NextReport { ready, blocked }
}

fn compare_next_order(a: &Task, b: &Task) -> std::cmp::Ordering {
    cmp_priority(&a.frontmatter.priority, &b.frontmatter.priority)
        .then_with(|| compare_created_at(a, b))
        .then_with(|| a.frontmatter.id.cmp(&b.frontmatter.id))
}

fn compare_created_at(a: &Task, b: &Task) -> std::cmp::Ordering {
    match (
        parse_timestamp(a.frontmatter.created_at.as_deref()),
        parse_timestamp(b.frontmatter.created_at.as_deref()),
    ) {
        (Some(a_created), Some(b_created)) => a_created.cmp(&b_created),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn parse_timestamp(value: Option<&str>) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    value.and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
}

fn is_candidate(
    task: &Task,
    sprint_task_ids: Option<&HashSet<String>>,
    include_backlog: bool,
) -> bool {
    let status_ok = if include_backlog {
        matches!(
            task.frontmatter.status,
            TaskStatus::Backlog | TaskStatus::Todo
        )
    } else {
        matches!(task.frontmatter.status, TaskStatus::Todo)
    };
    if !status_ok {
        return false;
    }
    if let Some(sprint_task_ids) = sprint_task_ids {
        return sprint_task_ids.contains(&task.frontmatter.id);
    }
    true
}

fn active_area_tasks(tasks: &[Task]) -> HashMap<String, Vec<String>> {
    let mut by_area: HashMap<String, Vec<String>> = HashMap::new();
    for task in tasks {
        if !matches!(task.frontmatter.status, TaskStatus::InProgress) {
            continue;
        }
        for area in &task.frontmatter.area {
            by_area
                .entry(area.clone())
                .or_default()
                .push(task.frontmatter.id.clone());
        }
    }
    by_area
}

fn area_conflicts(task: &Task, active_area_tasks: &HashMap<String, Vec<String>>) -> Vec<String> {
    task.frontmatter
        .area
        .iter()
        .filter_map(|area| active_area_tasks.get(area))
        .flatten()
        .cloned()
        .collect()
}

fn ordered_tasks<'a>(tasks: &'a [Task], sprints: &[Sprint], sprint: Option<&str>) -> Vec<&'a Task> {
    let task_by_id: HashMap<&str, &Task> = tasks
        .iter()
        .map(|task| (task.frontmatter.id.as_str(), task))
        .collect();
    let selected_sprint = sprint
        .and_then(|id| sprints.iter().find(|s| s.header.id == id))
        .or_else(|| latest_sprint(sprints));

    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    if let Some(sprint) = selected_sprint {
        for entry in &sprint.task_ids {
            let id = numeric_prefix(entry);
            if let Some(task) = task_by_id.get(id) {
                ordered.push(*task);
                seen.insert(task.frontmatter.id.as_str());
            }
        }
    }
    for task in tasks {
        if seen.insert(task.frontmatter.id.as_str()) {
            ordered.push(task);
        }
    }
    ordered
}

fn sprint_task_ids(sprints: &[Sprint], sprint: Option<&str>) -> Option<HashSet<String>> {
    let sprint = sprint.and_then(|id| sprints.iter().find(|s| s.header.id == id))?;
    Some(
        sprint
            .task_ids
            .iter()
            .map(|entry| numeric_prefix(entry).to_owned())
            .collect(),
    )
}

fn latest_sprint(sprints: &[Sprint]) -> Option<&Sprint> {
    sprints
        .iter()
        .max_by_key(|sprint| sprint_number(&sprint.header.id))
}

fn sprint_number(id: &str) -> u64 {
    id.trim_start_matches('s').parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;
    use crate::sprint::parse_sprint;

    fn task(id: &str, title: &str, status: &str, extra: &str) -> Task {
        let content =
            format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: {status}\n{extra}\n---\n");
        parse_task(&content, &format!("{id}-task.md")).unwrap()
    }

    #[test]
    fn ready_excludes_unfinished_local_blockers() {
        let tasks = vec![
            task("0001", "A", "todo", ""),
            task("0002", "B", "todo", "blocked_by: [\"0001\"]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert_eq!(
            report
                .ready
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0001"]
        );
        assert_eq!(report.blocked[0].id, "0002");
    }

    #[test]
    fn done_local_blockers_make_task_ready() {
        let tasks = vec![
            task("0001", "A", "done", ""),
            task("0002", "B", "todo", "blocked_by: [\"0001\"]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert_eq!(report.ready[0].id, "0002");
    }

    #[test]
    fn area_conflict_excludes_ready_task_by_default() {
        let tasks = vec![
            task("0001", "A", "in-progress", "area: [cli]"),
            task("0002", "B", "todo", "area: [cli]"),
            task("0003", "C", "todo", "area: [docs]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert_eq!(
            report
                .ready
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0003"]
        );
        assert_eq!(report.blocked[0].area_conflicts, vec!["0001"]);
    }

    #[test]
    fn area_conflict_with_selected_ready_task_is_attributed() {
        // No in-progress work: 0001 and 0002 share area cli. 0001 is selected
        // ready; 0002 is blocked, attributed to 0001 (not an in-progress task).
        let tasks = vec![
            task("0001", "A", "todo", "area: [cli]"),
            task("0002", "B", "todo", "area: [cli]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert_eq!(report.ready[0].id, "0001");
        assert_eq!(report.blocked[0].id, "0002");
        assert!(report.blocked[0].area_conflicts.is_empty());
        assert_eq!(report.blocked[0].selected_conflicts, vec!["0001"]);
    }

    #[test]
    fn sprint_selection_filters_without_overriding_priority() {
        let tasks = vec![
            task("0001", "A", "todo", "priority: p0"),
            task("0002", "B", "todo", "priority: p3"),
            task("0003", "C", "todo", "priority: p0"),
        ];
        let sprint = parse_sprint("# Sprint 1 · Jun 1-14\n\n- 0002\n- 0001\n").unwrap();
        let report = compute_next(
            &tasks,
            &[sprint],
            NextOptions {
                sprint: Some("s1"),
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert_eq!(
            report
                .ready
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0001", "0002"]
        );
    }

    #[test]
    fn priority_sorts_ready_tasks() {
        let tasks = vec![
            task("0001", "Low", "todo", "priority: p3"),
            task("0002", "High", "todo", "priority: p0"),
            task("0003", "NoPrio", "todo", ""),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        let ids: Vec<&str> = report.ready.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["0002", "0001", "0003"]);
    }

    #[test]
    fn created_at_breaks_priority_ties_oldest_first() {
        let tasks = vec![
            task(
                "0001",
                "Newer",
                "todo",
                "priority: p1\ncreated_at: \"2026-06-12T00:00:00Z\"",
            ),
            task(
                "0002",
                "Older",
                "todo",
                "priority: p1\ncreated_at: \"2026-06-10T00:00:00Z\"",
            ),
            task("0003", "Unstamped", "todo", "priority: p1"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        let ids: Vec<&str> = report.ready.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["0002", "0001", "0003"]);
    }

    #[test]
    fn priority_order_decides_parallel_area_selection() {
        let tasks = vec![
            task("0001", "Low", "todo", "priority: p3\narea: [cli]"),
            task("0002", "High", "todo", "priority: p1\narea: [cli]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert_eq!(report.ready[0].id, "0002");
        assert_eq!(report.blocked[0].id, "0001");
        assert_eq!(report.blocked[0].selected_conflicts, vec!["0002"]);
    }

    #[test]
    fn blocked_middle_task_stays_blocked_without_bottleneck_surface() {
        let tasks = vec![
            task("0001", "A", "in-progress", ""),
            task("0002", "B", "todo", "blocked_by: [\"0001\"]"),
            task("0003", "C", "todo", "blocked_by: [\"0002\"]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert!(report.ready.is_empty());
        assert_eq!(
            report
                .blocked
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0002", "0003"]
        );
    }

    #[test]
    fn backlog_excluded_from_ready_and_blocked_by_default() {
        let tasks = vec![
            task("0001", "A", "backlog", ""),
            task("0002", "B", "backlog", "blocked_by: [\"0001\"]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: false,
            },
        );
        assert!(report.ready.is_empty());
        assert!(report.blocked.is_empty());
    }

    #[test]
    fn backlog_included_when_flag_set() {
        let tasks = vec![
            task("0001", "A", "backlog", ""),
            task("0002", "B", "backlog", "blocked_by: [\"0001\"]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
                include_backlog: true,
            },
        );
        assert_eq!(report.ready[0].id, "0001");
        assert_eq!(report.blocked[0].id, "0002");
    }
}
