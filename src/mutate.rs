/// Pure mutation helpers for `Task` content.
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

/// Clear `started_at` (used by unclaim to return a task to a fresh todo state).
pub fn clear_started_at(task: &mut Task) {
    task.frontmatter.started_at = None;
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

/// Default markdown body template for a freshly created task.
pub const DEFAULT_TASK_BODY: &str = "## Why\n\n\n## Done When\n\n\n## References\n\n";

/// Required sections in a decision gate task body (`tags: ["gate"]`).
pub const REQUIRED_GATE_SECTIONS: &[&str] = &[
    "Ask",
    "Options",
    "Good answer",
    "Test",
    "Evidence",
    "Links",
    "Decision",
];

/// Default markdown body template for a decision gate.
pub const DEFAULT_GATE_BODY: &str = "\
## Ask\n\n\n\
## Options\n\n\n\
## Good answer\n\n\n\
## Test\n\nnone\n\n\
## Evidence\n\n\n\
## Links\n\n\n\
## Decision\n\n";

/// Build a structured task body from CLI flags (`--why`, `--done-when`, `--references`).
pub fn build_task_body(why: Option<&str>, done_when: &[String], references: &[String]) -> String {
    let why_text = why.map(str::trim).unwrap_or("");
    let done_items = format_bullet_list(done_when);
    let ref_items = format_bullet_list(references);

    let mut out = String::new();
    out.push_str("## Why\n\n");
    if !why_text.is_empty() {
        out.push_str(why_text);
        out.push('\n');
    }
    out.push_str("\n## Done When\n\n");
    if !done_items.is_empty() {
        out.push_str(&done_items);
        out.push('\n');
    }
    out.push_str("\n## References\n\n");
    if !ref_items.is_empty() {
        out.push_str(&ref_items);
        out.push('\n');
    }
    out
}

/// Build a structured gate body from CLI flags.
pub fn build_gate_body(
    ask: &str,
    options: &[String],
    good_answer: &str,
    test: Option<&str>,
    evidence: &str,
    links: &[String],
) -> String {
    let options_block = format_bullet_list(options);
    let links_block = format_bullet_list(links);
    let test_text = test
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("none");

    format!(
        "## Ask\n\n{}\n\n## Options\n\n{}\n\n## Good answer\n\n{}\n\n## Test\n\n{}\n\n## Evidence\n\n{}\n\n## Links\n\n{}\n\n## Decision\n\n",
        ask.trim(),
        options_block,
        good_answer.trim(),
        test_text,
        evidence.trim(),
        links_block,
    )
}

fn format_bullet_list(items: &[String]) -> String {
    items
        .iter()
        .flat_map(|item| item.lines())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with("- ") || s.starts_with("* ") {
                s.to_owned()
            } else {
                format!("- {s}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Append blocker references to `task.frontmatter.blocked_by`, avoiding duplicates.
pub fn append_blockers(task: &mut Task, new_refs: Vec<crate::schema::BlockedByRef>) {
    for r in new_refs {
        if !task.frontmatter.blocked_by.contains(&r) {
            task.frontmatter.blocked_by.push(r);
        }
    }
}

/// Remove matching blocker references from `task.frontmatter.blocked_by`.
pub fn remove_blockers(task: &mut Task, to_remove: &[crate::schema::BlockedByRef]) {
    task.frontmatter
        .blocked_by
        .retain(|existing| !to_remove.contains(existing));
}

/// Build the initial file content for a new task.
///
/// Produces valid frontmatter with the minimum required fields.  `$EDITOR`
/// will be opened on this content so the user can fill in the body.
pub fn new_task_content(id: &str, title: &str, created_at: Option<&str>) -> String {
    let created_at = created_at
        .map(|timestamp| format!("created_at: \"{timestamp}\"\n"))
        .unwrap_or_default();
    format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: backlog\n{created_at}---\n\n{DEFAULT_TASK_BODY}")
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
}
