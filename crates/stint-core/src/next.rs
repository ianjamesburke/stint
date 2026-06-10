use std::collections::{HashMap, HashSet};

use crate::schema::{Sprint, Task, TaskStatus};
use crate::sprint::numeric_prefix;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextOptions<'a> {
    pub sprint: Option<&'a str>,
    pub include_area_conflicts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextTask {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub sprint: Option<String>,
    pub area: Vec<String>,
    pub blockers: Vec<String>,
    pub area_conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bottleneck {
    pub id: String,
    pub title: String,
    pub blocked_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextReport {
    pub ready: Vec<NextTask>,
    pub blocked: Vec<NextTask>,
    pub bottleneck: Option<Bottleneck>,
}

pub fn compute_next(tasks: &[Task], sprints: &[Sprint], options: NextOptions<'_>) -> NextReport {
    let task_by_id: HashMap<&str, &Task> = tasks
        .iter()
        .map(|task| (task.frontmatter.id.as_str(), task))
        .collect();
    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|task| matches!(task.frontmatter.status, TaskStatus::Done))
        .map(|task| task.frontmatter.id.as_str())
        .collect();
    let active_area_tasks = active_area_tasks(tasks);
    let sprint_task_ids = sprint_task_ids(sprints, options.sprint);
    let ordered = ordered_tasks(tasks, sprints, options.sprint);

    let mut ready = Vec::new();
    let mut blocked = Vec::new();
    let mut claimed_areas = HashSet::new();

    for task in ordered {
        if !is_candidate(task, sprint_task_ids.as_ref()) {
            continue;
        }

        let blockers = blockers(task, &done_ids);
        let mut area_conflicts = area_conflicts(task, &active_area_tasks);
        area_conflicts.sort();
        area_conflicts.dedup();

        let row = NextTask {
            id: task.frontmatter.id.clone(),
            title: task.frontmatter.title.clone(),
            status: task.frontmatter.status.clone(),
            sprint: task.frontmatter.sprint.clone(),
            area: task.frontmatter.area.clone(),
            blockers: blockers.clone(),
            area_conflicts: area_conflicts.clone(),
        };

        if !blockers.is_empty() {
            blocked.push(row);
            continue;
        }

        let conflicts_with_selected = task
            .frontmatter
            .area
            .iter()
            .any(|area| claimed_areas.contains(area));
        if !options.include_area_conflicts
            && (!area_conflicts.is_empty() || conflicts_with_selected)
        {
            blocked.push(row);
            continue;
        }

        for area in &task.frontmatter.area {
            claimed_areas.insert(area.clone());
        }
        ready.push(row);
    }

    NextReport {
        ready,
        blocked,
        bottleneck: bottleneck(tasks, &task_by_id),
    }
}

fn is_candidate(task: &Task, sprint_task_ids: Option<&HashSet<String>>) -> bool {
    if !matches!(
        task.frontmatter.status,
        TaskStatus::Backlog | TaskStatus::Todo
    ) {
        return false;
    }
    if let Some(sprint_task_ids) = sprint_task_ids {
        return sprint_task_ids.contains(&task.frontmatter.id);
    }
    true
}

fn blockers(task: &Task, done_ids: &HashSet<&str>) -> Vec<String> {
    use crate::schema::BlockedByRef;
    let mut blockers = Vec::new();
    for r in &task.frontmatter.blocked_by {
        match r {
            BlockedByRef::LocalTask(id) => {
                if !done_ids.contains(id.as_str()) {
                    blockers.push(r.to_string());
                }
            }
            // Non-local refs are always active blockers (can't resolve locally).
            other => blockers.push(other.to_string()),
        }
    }
    blockers
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

fn bottleneck(tasks: &[Task], task_by_id: &HashMap<&str, &Task>) -> Option<Bottleneck> {
    use crate::schema::BlockedByRef;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for task in tasks {
        if matches!(
            task.frontmatter.status,
            TaskStatus::Done | TaskStatus::Archived
        ) {
            continue;
        }
        for blocker in &task.frontmatter.blocked_by {
            if let BlockedByRef::LocalTask(id) = blocker {
                *counts.entry(id.as_str()).or_default() += 1;
            }
        }
    }

    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .and_then(|(id, count)| {
            task_by_id.get(id).map(|task| Bottleneck {
                id: task.frontmatter.id.clone(),
                title: task.frontmatter.title.clone(),
                blocked_count: count,
            })
        })
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
    fn sprint_order_wins() {
        let tasks = vec![task("0001", "A", "todo", ""), task("0002", "B", "todo", "")];
        let sprint = parse_sprint("# Sprint 1 · Jun 1-14\n\n- 0002\n- 0001\n").unwrap();
        let report = compute_next(
            &tasks,
            &[sprint],
            NextOptions {
                sprint: Some("s1"),
                include_area_conflicts: false,
            },
        );
        assert_eq!(
            report
                .ready
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0002", "0001"]
        );
    }

    #[test]
    fn bottleneck_counts_direct_dependents() {
        let tasks = vec![
            task("0001", "A", "todo", ""),
            task("0002", "B", "todo", "blocked_by: [\"0001\"]"),
            task("0003", "C", "todo", "blocked_by: [\"0001\"]"),
        ];
        let report = compute_next(
            &tasks,
            &[],
            NextOptions {
                sprint: None,
                include_area_conflicts: false,
            },
        );
        assert_eq!(report.bottleneck.unwrap().blocked_count, 2);
    }
}
