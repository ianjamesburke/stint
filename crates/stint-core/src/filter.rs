/// Task filtering predicates extracted from the CLI so they can be tested
/// independently of the I/O layer.
use crate::schema::Task;

/// Return references to tasks that match all of the supplied filters.
///
/// A `None` filter is a wildcard (matches everything).  All non-`None` filters
/// are ANDed together.
pub fn filter_tasks<'a>(
    tasks: &'a [Task],
    status: Option<&str>,
    sprint: Option<&str>,
    area: Option<&str>,
    tag: Option<&str>,
) -> Vec<&'a Task> {
    tasks
        .iter()
        .filter(|t| {
            if let Some(s) = status {
                if t.frontmatter.status.as_str() != s {
                    return false;
                }
            }
            if let Some(sp) = sprint {
                match &t.frontmatter.sprint {
                    Some(ts) if ts == sp => {}
                    _ => return false,
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

    fn make_task(id: &str, title: &str, status: &str) -> Task {
        let content = format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: {status}\n---\n");
        let filename = format!("{id}.md");
        parse_task(&content, &filename).unwrap()
    }

    #[test]
    fn filter_all_none_returns_all() {
        let tasks = vec![
            make_task("0001", "A", "backlog"),
            make_task("0002", "B", "done"),
        ];
        let result = filter_tasks(&tasks, None, None, None, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_by_status() {
        let tasks = vec![
            make_task("0001", "A", "backlog"),
            make_task("0002", "B", "done"),
        ];
        let result = filter_tasks(&tasks, Some("backlog"), None, None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].frontmatter.id, "0001");
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let tasks = vec![make_task("0001", "A", "backlog")];
        let result = filter_tasks(&tasks, Some("done"), None, None, None);
        assert!(result.is_empty());
    }
}
