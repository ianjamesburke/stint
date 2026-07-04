/// Pure mutation helpers for `Task` and sprint content.
///
/// All functions are side-effect-free — callers are responsible for writing
/// results back to disk.
use crate::duration::Duration;
use crate::schema::{Task, TaskFrontmatter, TaskStatus};

// ---------------------------------------------------------------------------
// Task field mutations
// ---------------------------------------------------------------------------

/// Set the status of a task.
pub fn set_status(task: &mut Task, status: TaskStatus) {
    task.frontmatter.status = status;
}

/// Add `duration` to the task's `actual` field, accumulating from any
/// existing value.
pub fn add_actual(task: &mut Task, duration: Duration) {
    let existing = task.frontmatter.actual.unwrap_or(Duration::from_minutes(0));
    task.frontmatter.actual = Some(existing + duration);
}

/// Replace the task's `actual` field.
pub fn set_actual(task: &mut Task, duration: Duration) {
    task.frontmatter.actual = Some(duration);
}

/// Set `started_at` unless it is already present.
pub fn set_started_at_if_absent(task: &mut Task, timestamp: String) {
    if task.frontmatter.started_at.is_none() {
        task.frontmatter.started_at = Some(timestamp);
    }
}

/// Replace `started_at` and clear completion fields for a fresh run.
pub fn restart_task(task: &mut Task, timestamp: String) {
    task.frontmatter.started_at = Some(timestamp);
    task.frontmatter.completed_at = None;
    task.frontmatter.actual = None;
}

/// Set `completed_at`.
pub fn set_completed_at(task: &mut Task, timestamp: String) {
    task.frontmatter.completed_at = Some(timestamp);
}

// ---------------------------------------------------------------------------
// ID helpers
// ---------------------------------------------------------------------------

/// Compute the next available task ID given a list of existing tasks.
///
/// Finds the highest numeric ID and returns it incremented by 1, zero-padded
/// to 4 digits.  Returns `"0001"` when no tasks exist.
pub fn next_task_id(tasks: &[Task]) -> String {
    let max = tasks
        .iter()
        .filter_map(|t| t.frontmatter.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{:04}", max + 1)
}

/// Resolve a user-supplied task ID fragment to a canonical 4-digit ID.
///
/// Handles:
/// - Full `"0001"` — returned as-is after zero-padding.
/// - Partial `"1"` — padded to `"0001"`.
/// - With slug `"0001-auth-middleware"` — slug stripped, numeric prefix used.
pub fn resolve_id(input: &str) -> String {
    // Take the leading digit run.
    let numeric: String = input.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u32 = numeric.parse().unwrap_or(0);
    format!("{:04}", n)
}

// ---------------------------------------------------------------------------
// New task content
// ---------------------------------------------------------------------------

/// Convert a task title to a filesystem-safe slug.
///
/// `"Auth Middleware"` → `"auth-middleware"`.
pub fn title_to_slug(title: &str) -> String {
    let raw: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse consecutive dashes and trim leading/trailing.
    raw.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Build the initial file content for a new task.
///
/// Produces valid frontmatter with the minimum required fields.  `$EDITOR`
/// will be opened on this content so the user can fill in the body.
pub fn new_task_content(id: &str, title: &str, created_at: Option<&str>) -> String {
    let created_at = created_at
        .map(|timestamp| format!("created_at: \"{timestamp}\"\n"))
        .unwrap_or_default();
    format!(
        "---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: backlog\n{created_at}---\n\n## Why\n\n\n## Gotchas\n\n\n## References\n\n"
    )
}

/// Build the initial content for a new sprint file.
pub fn new_sprint_content(id: &str, date_range: &str, goal: Option<&str>) -> String {
    // id is like "s12" — extract the number for the header.
    let number = id.trim_start_matches('s');
    let mut header = format!("# Sprint {number} \u{00B7} {date_range}");
    if let Some(g) = goal {
        header.push_str(&format!(" \u{00B7} goal: {g}"));
    }
    header.push('\n');
    header
}

/// Build a minimal `TaskFrontmatter` for a new task (no optional fields set).
pub fn minimal_frontmatter(id: &str, title: &str) -> TaskFrontmatter {
    TaskFrontmatter {
        id: id.to_owned(),
        title: title.to_owned(),
        status: TaskStatus::Backlog,
        priority: None,
        size: None,
        estimate: None,
        actual: None,
        created_at: None,
        started_at: None,
        completed_at: None,
        blocked_by: vec![],
        gh_issue: vec![],
        area: vec![],
        tags: vec![],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;

    fn make_task(id: &str, title: &str) -> Task {
        let filename = format!("{}-slug.md", id);
        let content = format!(
            "---\nid: \"{}\"\ntitle: \"{}\"\nstatus: backlog\n---\n",
            id, title
        );
        parse_task(&content, &filename).unwrap()
    }

    #[test]
    fn set_status_done() {
        let mut task = make_task("0001", "Task A");
        set_status(&mut task, TaskStatus::Done);
        assert_eq!(task.frontmatter.status, TaskStatus::Done);
    }

    #[test]
    fn add_actual_accumulates() {
        let mut task = make_task("0001", "Task A");
        add_actual(&mut task, Duration::from_minutes(60));
        add_actual(&mut task, Duration::from_minutes(30));
        assert_eq!(task.frontmatter.actual, Some(Duration::from_minutes(90)));
    }

    #[test]
    fn add_actual_from_zero() {
        let mut task = make_task("0001", "Task A");
        assert!(task.frontmatter.actual.is_none());
        add_actual(&mut task, Duration::from_minutes(45));
        assert_eq!(task.frontmatter.actual, Some(Duration::from_minutes(45)));
    }

    #[test]
    fn next_id_empty() {
        assert_eq!(next_task_id(&[]), "0001");
    }

    #[test]
    fn next_id_increments() {
        let tasks = vec![make_task("0001", "A"), make_task("0003", "B")];
        assert_eq!(next_task_id(&tasks), "0004");
    }

    #[test]
    fn resolve_id_full() {
        assert_eq!(resolve_id("0001"), "0001");
    }

    #[test]
    fn resolve_id_partial() {
        assert_eq!(resolve_id("1"), "0001");
        assert_eq!(resolve_id("12"), "0012");
    }

    #[test]
    fn resolve_id_with_slug() {
        assert_eq!(resolve_id("0001-auth-middleware"), "0001");
    }

    #[test]
    fn title_to_slug_basic() {
        assert_eq!(title_to_slug("Auth Middleware"), "auth-middleware");
    }

    #[test]
    fn title_to_slug_collapses_dashes() {
        assert_eq!(title_to_slug("foo  bar"), "foo-bar");
    }

    #[test]
    fn new_sprint_content_with_goal() {
        let content = new_sprint_content("s12", "Jun 9-20", Some("ship TUI"));
        assert!(content.starts_with("# Sprint 12"));
        assert!(content.contains("goal: ship TUI"));
    }

    #[test]
    fn new_sprint_content_no_goal() {
        let content = new_sprint_content("s1", "Jan 1-15", None);
        assert!(content.starts_with("# Sprint 1"));
        assert!(!content.contains("goal"));
    }
}
