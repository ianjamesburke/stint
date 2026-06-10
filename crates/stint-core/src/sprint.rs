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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a sprint index file from its string content.
///
/// The sprint ID is derived from the header line (e.g. `# Sprint 12` → `"s12"`).
pub fn parse_sprint(content: &str) -> Result<Sprint, SprintParseError> {
    let mut lines = content.lines();

    let header_line = lines
        .next()
        .ok_or(SprintParseError::InvalidHeader)?;

    let header = parse_sprint_header(header_line)?;

    let task_ids: Vec<String> = lines
        .filter_map(|line| parse_task_list_entry(line))
        .collect();

    Ok(Sprint { header, task_ids })
}

/// Append a task entry to a sprint file's string content.
///
/// Returns the new file content with the task ID appended to the list.
pub fn sprint_add_task(content: &str, task_id: &str) -> String {
    let trimmed = content.trim_end_matches('\n');
    format!("{}\n- {}\n", trimmed, task_id)
}

/// Remove a task entry from a sprint file's string content by numeric prefix.
///
/// Matches entries where the numeric prefix equals `task_id`.  Returns the
/// updated content.  If no match is found the content is returned unchanged.
pub fn sprint_remove_task(content: &str, task_id: &str) -> String {
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            if let Some(entry) = parse_task_list_entry(line) {
                numeric_prefix(&entry) != task_id
            } else {
                true
            }
        })
        .collect();

    if lines.is_empty() {
        String::new()
    } else {
        let mut result = lines.join("\n");
        result.push('\n');
        result
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

    // At minimum we need the "Sprint <N>" part
    if parts.is_empty() {
        return Err(SprintParseError::InvalidHeader);
    }

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

/// Extract the numeric prefix of a task entry.
///
/// `"0001-auth-middleware"` → `"0001"`, `"0001"` → `"0001"`.
pub fn numeric_prefix(entry: &str) -> &str {
    match entry.find('-') {
        Some(pos) => &entry[..pos],
        None => entry,
    }
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
        let updated = sprint_add_task(SPRINT_FILE, "0010-new-task");
        let sprint = parse_sprint(&updated).unwrap();
        assert_eq!(sprint.task_ids.last().unwrap(), "0010-new-task");
        assert_eq!(sprint.task_ids.len(), 5);
    }

    #[test]
    fn sprint_remove_task_by_id() {
        let updated = sprint_remove_task(SPRINT_FILE, "0004");
        let sprint = parse_sprint(&updated).unwrap();
        assert!(!sprint.task_ids.iter().any(|id| id.starts_with("0004")));
        assert_eq!(sprint.task_ids.len(), 3);
    }

    #[test]
    fn sprint_remove_nonexistent_is_noop() {
        let updated = sprint_remove_task(SPRINT_FILE, "9999");
        let sprint_before = parse_sprint(SPRINT_FILE).unwrap();
        let sprint_after = parse_sprint(&updated).unwrap();
        assert_eq!(sprint_before.task_ids.len(), sprint_after.task_ids.len());
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
