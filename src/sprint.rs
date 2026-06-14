/// Parse and mutate sprint index files.
///
/// Sprint files use a fixed header format followed by an ordered task list:
///
/// ```text
/// # Sprint 12 · Jun 9–20 · goal: ship TUI skeleton
///
/// - 0001-auth-middleware
/// - 0004-tui-skeleton
/// ```
use thiserror::Error;

use crate::schema::{Sprint, SprintHeader};

/// Errors that can occur while parsing a sprint file.
#[derive(Debug, Error, PartialEq)]
pub enum SprintParseError {
    /// The file does not begin with a valid `# Sprint …` header line.
    #[error("missing or invalid sprint header line")]
    InvalidHeader,
}

/// Errors that can occur when adding a task to a sprint.
#[derive(Debug, Error, PartialEq)]
pub enum SprintAddError {
    /// The task is already present in the sprint.
    #[error("task {0:?} is already in the sprint")]
    AlreadyPresent(String),
}

/// Errors that can occur when removing a task from a sprint.
#[derive(Debug, Error, PartialEq)]
pub enum SprintRemoveError {
    /// The task was not found in the sprint.
    #[error("task {0:?} is not in the sprint")]
    NotFound(String),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a sprint index file from its string content.
///
/// The sprint ID is derived from the header line (e.g. `# Sprint 12` → `"s12"`).
pub fn parse_sprint(content: &str) -> Result<Sprint, SprintParseError> {
    let mut lines = content.lines();

    let header_line = lines.next().ok_or(SprintParseError::InvalidHeader)?;

    let header = parse_sprint_header(header_line)?;

    let task_ids: Vec<String> = lines
        .filter_map(|line| parse_task_list_entry(line))
        .collect();

    Ok(Sprint { header, task_ids })
}

/// Append a task entry to a sprint file's string content.
///
/// Returns the new file content with the task ID appended to the list, or
/// `Err(SprintAddError::AlreadyPresent)` if the task is already in the sprint.
/// `entry` is the literal text appended after `- ` (a bare ID or a
/// `../tasks/<filename>` link). Duplicate detection is by task ID, so the same
/// task cannot be added twice regardless of entry form.
pub fn sprint_add_task(content: &str, entry: &str) -> Result<String, SprintAddError> {
    let new_id = numeric_prefix(entry);
    let already_present = content
        .lines()
        .filter_map(|line| parse_task_list_entry(line))
        .any(|existing| numeric_prefix(&existing) == new_id);
    if already_present {
        return Err(SprintAddError::AlreadyPresent(new_id.to_owned()));
    }
    let trimmed = content.trim_end_matches('\n');
    Ok(format!("{}\n- {}\n", trimmed, entry))
}

/// Remove a task entry from a sprint file's string content by numeric prefix.
///
/// Matches entries where the numeric prefix equals `task_id`.  Returns the
/// updated content, or `Err(SprintRemoveError::NotFound)` if the task is not
/// present in the sprint.
pub fn sprint_remove_task(content: &str, task_id: &str) -> Result<String, SprintRemoveError> {
    let mut removed = false;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            if let Some(entry) = parse_task_list_entry(line) {
                if numeric_prefix(&entry) == task_id {
                    removed = true;
                    return false;
                }
            }
            true
        })
        .collect();

    if !removed {
        return Err(SprintRemoveError::NotFound(task_id.to_owned()));
    }

    if lines.is_empty() {
        Ok(String::new())
    } else {
        let mut result = lines.join("\n");
        result.push('\n');
        Ok(result)
    }
}

/// Normalise a sprint ID: `"12"` → `"s12"`, `"s12"` → `"s12"`.
pub fn normalize_sprint_id(id: &str) -> String {
    if id.starts_with('s') {
        id.to_owned()
    } else {
        format!("s{}", id)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse the header line into a `SprintHeader`.
fn parse_sprint_header(line: &str) -> Result<SprintHeader, SprintParseError> {
    // Expected: "# Sprint <number> · <date-range> · goal: <goal>"
    // The separator is " · " (space + U+00B7 MIDDLE DOT + space).
    let without_hash = line
        .strip_prefix("# ")
        .ok_or(SprintParseError::InvalidHeader)?;

    // Split on " · " (middle dot separator)
    let parts: Vec<&str> = without_hash.split(" \u{00B7} ").collect();

    let sprint_part = parts[0];
    let number_str = sprint_part
        .strip_prefix("Sprint ")
        .ok_or(SprintParseError::InvalidHeader)?;
    let number: u64 = number_str
        .trim()
        .parse()
        .map_err(|_| SprintParseError::InvalidHeader)?;

    let id = format!("s{}", number);

    let date_range = parts
        .get(1)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();

    let goal = parts
        .get(2)
        .and_then(|s| s.trim().strip_prefix("goal:"))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    Ok(SprintHeader {
        id,
        date_range,
        goal,
    })
}

/// Extract the task entry from a list line `- <entry>`, returning `None` for
/// lines that are not task list entries.
fn parse_task_list_entry(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let entry = trimmed.strip_prefix("- ")?;
    let entry = entry.trim();
    if entry.is_empty() {
        None
    } else {
        Some(entry.to_owned())
    }
}

/// Extract the task ID from a sprint entry, tolerant of every accepted form.
///
/// Markdown link `"[0001](../tasks/0001-auth.md)"` → `"0001"`, plain path
/// `"../tasks/0001-auth.md"` → `"0001"`, `"0001-auth"` → `"0001"`,
/// `"0001"` → `"0001"`.
pub fn numeric_prefix(entry: &str) -> &str {
    // For a markdown link `[label](target)`, read the ID from the target.
    let core = match entry.find("](") {
        Some(open) => {
            let target = &entry[open + 2..];
            target.strip_suffix(')').unwrap_or(target)
        }
        None => entry,
    };
    // Drop any directory portion ("../tasks/0001-x.md" → "0001-x.md").
    let base = core.rsplit('/').next().unwrap_or(core);
    // Drop the ".md" extension if present.
    let base = base.strip_suffix(".md").unwrap_or(base);
    // The ID is everything before the first '-'.
    match base.find('-') {
        Some(pos) => &base[..pos],
        None => base,
    }
}

/// The canonical sprint-file entry for a task: a markdown link from
/// `.stint/sprints/` to `.stint/tasks/<filename>`. Markdown link syntax makes
/// it a real, clickable link — cmd-click in VS Code, `gf`/link-follow in Vim.
pub fn task_link(filename: &str) -> String {
    format!("[{}](../tasks/{})", numeric_prefix(filename), filename)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SPRINT_FILE: &str = r#"# Sprint 12 · Jun 9–20 · goal: ship TUI skeleton

- 0001-auth-middleware
- 0004-tui-skeleton
- 0007-gh-import
- 0003-docs
"#;

    #[test]
    fn parse_full_sprint() {
        let sprint = parse_sprint(SPRINT_FILE).unwrap();
        assert_eq!(sprint.header.id, "s12");
        assert_eq!(sprint.header.date_range, "Jun 9–20");
        assert_eq!(sprint.header.goal.as_deref(), Some("ship TUI skeleton"));
        assert_eq!(
            sprint.task_ids,
            vec![
                "0001-auth-middleware",
                "0004-tui-skeleton",
                "0007-gh-import",
                "0003-docs"
            ]
        );
    }

    #[test]
    fn parse_sprint_no_goal() {
        let content = "# Sprint 3 · Jul 1–14\n\n- 0001\n";
        let sprint = parse_sprint(content).unwrap();
        assert_eq!(sprint.header.id, "s3");
        assert_eq!(sprint.header.date_range, "Jul 1–14");
        assert!(sprint.header.goal.is_none());
        assert_eq!(sprint.task_ids, vec!["0001"]);
    }

    #[test]
    fn parse_sprint_header_only() {
        let content = "# Sprint 1 · Jan 1–15 · goal: MVP\n";
        let sprint = parse_sprint(content).unwrap();
        assert_eq!(sprint.header.id, "s1");
        assert!(sprint.task_ids.is_empty());
    }

    #[test]
    fn sprint_add_task_appends() {
        let updated = sprint_add_task(SPRINT_FILE, "0010-new-task").unwrap();
        let sprint = parse_sprint(&updated).unwrap();
        assert_eq!(sprint.task_ids.last().unwrap(), "0010-new-task");
        assert_eq!(sprint.task_ids.len(), 5);
    }

    #[test]
    fn sprint_add_task_duplicate_errors() {
        // "0001" is already in SPRINT_FILE.
        let err = sprint_add_task(SPRINT_FILE, "0001").unwrap_err();
        assert!(matches!(err, SprintAddError::AlreadyPresent(_)));
    }

    #[test]
    fn sprint_add_link_entry_dedupes_by_id() {
        // A markdown-link entry for a task already present (by bare id) is rejected.
        let err = sprint_add_task(SPRINT_FILE, "[0001](../tasks/0001-auth.md)").unwrap_err();
        assert!(matches!(err, SprintAddError::AlreadyPresent(_)));
    }

    #[test]
    fn numeric_prefix_handles_all_entry_forms() {
        assert_eq!(numeric_prefix("0001"), "0001");
        assert_eq!(numeric_prefix("0001-auth-middleware"), "0001");
        assert_eq!(numeric_prefix("../tasks/0001-auth-middleware.md"), "0001");
        assert_eq!(numeric_prefix("tasks/0042-x.md"), "0042");
        assert_eq!(
            numeric_prefix("[0001](../tasks/0001-auth-middleware.md)"),
            "0001"
        );
    }

    #[test]
    fn task_link_is_a_markdown_link() {
        assert_eq!(
            task_link("0001-auth-middleware.md"),
            "[0001](../tasks/0001-auth-middleware.md)"
        );
    }

    #[test]
    fn sprint_remove_task_by_id() {
        let updated = sprint_remove_task(SPRINT_FILE, "0004").unwrap();
        let sprint = parse_sprint(&updated).unwrap();
        assert!(!sprint.task_ids.iter().any(|id| id.starts_with("0004")));
        assert_eq!(sprint.task_ids.len(), 3);
    }

    #[test]
    fn sprint_remove_nonexistent_errors() {
        let err = sprint_remove_task(SPRINT_FILE, "9999").unwrap_err();
        assert!(matches!(err, SprintRemoveError::NotFound(_)));
    }

    #[test]
    fn normalize_sprint_id_with_prefix() {
        assert_eq!(normalize_sprint_id("s12"), "s12");
    }

    #[test]
    fn normalize_sprint_id_without_prefix() {
        assert_eq!(normalize_sprint_id("12"), "s12");
    }

    #[test]
    fn numeric_prefix_with_slug() {
        assert_eq!(numeric_prefix("0001-auth-middleware"), "0001");
    }

    #[test]
    fn numeric_prefix_without_slug() {
        assert_eq!(numeric_prefix("0001"), "0001");
    }

    #[test]
    fn invalid_header() {
        assert!(matches!(
            parse_sprint("Not a sprint file\n"),
            Err(SprintParseError::InvalidHeader)
        ));
    }
}
