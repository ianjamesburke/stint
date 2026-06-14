/// Task filtering predicates extracted from the CLI so they can be tested
/// independently of the I/O layer.
use std::collections::HashSet;

use crate::schema::{Sprint, Task};
use crate::sprint::numeric_prefix;

/// Return references to tasks that match all of the supplied filters.
///
/// A `None` filter is a wildcard (matches everything).  All non-`None` filters
/// are ANDed together.
///
/// Sprint membership is determined by the sprint index files, not by any
/// `sprint` field on the task frontmatter.
pub fn filter_tasks<'a>(
    tasks: &'a [Task],
    sprints: &[Sprint],
    status: Option<&str>,
    sprint: Option<&str>,
    area: Option<&str>,
    tag: Option<&str>,
) -> Vec<&'a Task> {
    // Build the set of task IDs listed in the requested sprint (if any).
    // When a sprint filter is given but no matching sprint is found, we treat it
    // as "empty set" (no tasks match) rather than "no filter" (all tasks match).
    let sprint_task_ids: Option<HashSet<String>> = sprint.map(|sp| {
        sprints
            .iter()
            .find(|s| s.header.id == sp)
            .map(|s| s.task_ids.iter().map(|e| numeric_prefix(e).to_owned()).collect())
            .unwrap_or_default()
    });

    tasks
        .iter()
        .filter(|t| {
            if let Some(s) = status {
                if t.frontmatter.status.as_str() != s {
                    return false;
                }
            }
            if let Some(ref ids) = sprint_task_ids {
                if !ids.contains(t.frontmatter.id.as_str()) {
                    return false;
                }
            }
            if let Some(a) = area {
                if !t.frontmatter.area.iter().any(|x| x == a) {
                    return false;
                }
            }
            if let Some(tag) = tag {
                if !t.frontmatter.tags.iter().any(|x| x == tag) {
                    return false;
                }
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;
    use crate::sprint::parse_sprint;

    fn make_task(id: &str, title: &str, status: &str) -> Task {
        let content = format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: {status}\n---\n");
        let filename = format!("{id}.md");
        parse_task(&content, &filename).unwrap()
    }

    fn make_sprint(number: u32, task_ids: &[&str]) -> crate::schema::Sprint {
        let entries: String = task_ids.iter().map(|id| format!("- {}\n", id)).collect();
        let content = format!("# Sprint {} · Jan 1–15\n\n{}", number, entries);
        parse_sprint(&content).unwrap()
    }

    #[test]
    fn filter_all_none_returns_all() {
        let tasks = vec![
            make_task("0001", "A", "backlog"),
            make_task("0002", "B", "done"),
        ];
        let result = filter_tasks(&tasks, &[], None, None, None, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_by_status() {
        let tasks = vec![
            make_task("0001", "A", "backlog"),
            make_task("0002", "B", "done"),
        ];
        let result = filter_tasks(&tasks, &[], Some("backlog"), None, None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].frontmatter.id, "0001");
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let tasks = vec![make_task("0001", "A", "backlog")];
        let result = filter_tasks(&tasks, &[], Some("done"), None, None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_by_sprint_uses_index() {
        let tasks = vec![
            make_task("0001", "A", "backlog"),
            make_task("0002", "B", "todo"),
        ];
        let sprint = make_sprint(1, &["0001"]);
        let result = filter_tasks(&tasks, &[sprint], None, Some("s1"), None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].frontmatter.id, "0001");
    }

    #[test]
    fn filter_by_unknown_sprint_returns_empty() {
        let tasks = vec![make_task("0001", "A", "backlog")];
        let result = filter_tasks(&tasks, &[], None, Some("s99"), None, None);
        assert!(result.is_empty());
    }
}
