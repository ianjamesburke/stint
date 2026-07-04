/// Serialize `Task` and `Sprint` values back to their on-disk string formats.
use crate::schema::{BlockedByRef, Sprint, Task};

/// Render a `Task` back to a markdown file string (frontmatter + body).
///
/// The output round-trips through `parse::parse_task` — that is, parsing the
/// returned string produces an equivalent `Task`.
pub fn serialize_task(task: &Task) -> String {
    let fm = &task.frontmatter;
    let mut out = String::from("---\n");

    out.push_str(&format!("id: \"{}\"\n", fm.id));
    out.push_str(&format!("title: {}\n", yaml_quote(&fm.title)));
    out.push_str(&format!("status: {}\n", fm.status));
    if let Some(p) = &fm.priority {
        out.push_str(&format!("priority: {}\n", p));
    }
    if let Some(s) = &fm.size {
        out.push_str(&format!("size: {}\n", s));
    }

    if let Some(e) = &fm.estimate {
        out.push_str(&format!("estimate: \"{}\"\n", e));
    }
    if let Some(a) = &fm.actual {
        out.push_str(&format!("actual: \"{}\"\n", a));
    }
    if let Some(created_at) = &fm.created_at {
        out.push_str(&format!("created_at: {}\n", yaml_quote(created_at)));
    }
    if let Some(started_at) = &fm.started_at {
        out.push_str(&format!("started_at: {}\n", yaml_quote(started_at)));
    }
    if let Some(completed_at) = &fm.completed_at {
        out.push_str(&format!("completed_at: {}\n", yaml_quote(completed_at)));
    }

    write_blocker_list(&mut out, "blocked_by", &fm.blocked_by);

    write_string_list(&mut out, "gh_issue", &fm.gh_issue);
    write_string_list(&mut out, "area", &fm.area);
    write_string_list(&mut out, "tags", &fm.tags);

    out.push_str("---\n");

    if !task.body.is_empty() {
        out.push('\n');
        out.push_str(&task.body);
        if !task.body.ends_with('\n') {
            out.push('\n');
        }
    }

    out
}

/// Render a `Sprint` back to its markdown file string.
pub fn serialize_sprint(sprint: &Sprint) -> String {
    let number = sprint.header.id.trim_start_matches('s');
    let mut header = format!("# Sprint {}", number);
    if !sprint.header.date_range.is_empty() {
        header.push_str(&format!(" \u{00B7} {}", sprint.header.date_range));
    }
    if let Some(goal) = &sprint.header.goal {
        header.push_str(&format!(" \u{00B7} goal: {}", goal));
    }

    let mut out = header;
    out.push('\n');

    if !sprint.task_ids.is_empty() {
        out.push('\n');
        for id in &sprint.task_ids {
            out.push_str(&format!("- {}\n", id));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write the `blocked_by` YAML sequence. `LocalTask` items are emitted as bare
/// integers; all other variants are emitted as quoted strings.
fn write_blocker_list(out: &mut String, key: &str, values: &[BlockedByRef]) {
    if values.is_empty() {
        out.push_str(&format!("{}: []\n", key));
        return;
    }
    out.push_str(&format!("{}:\n", key));
    for v in values {
        match v {
            // LocalTask serializes as a bare integer so it round-trips
            // through the YAML integer parse path.
            BlockedByRef::LocalTask(id) => {
                let n: u64 = id.parse().unwrap_or(0);
                out.push_str(&format!("  - {}\n", n));
            }
            other => {
                out.push_str(&format!("  - {}\n", yaml_quote(&other.to_string())));
            }
        }
    }
}

/// Write a YAML sequence field. Emits `[]` for empty lists.
fn write_string_list(out: &mut String, key: &str, values: &[String]) {
    if values.is_empty() {
        out.push_str(&format!("{}: []\n", key));
    } else {
        out.push_str(&format!("{}:\n", key));
        for v in values {
            out.push_str(&format!("  - {}\n", yaml_quote(v)));
        }
    }
}

/// Wrap a string in YAML double-quotes.  Escapes backslashes and double-quotes
/// only — sufficient for the values that appear in stint frontmatter.
fn yaml_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;
    use crate::schema::BlockedByRef;
    use crate::sprint::parse_sprint;

    const FULL_TASK: &str = r#"---
id: "0001"
title: "Auth middleware"
status: in-progress
estimate: "4h"
actual: "30m"
created_at: "2026-06-10T11:00:00Z"
started_at: "2026-06-10T12:00:00Z"
completed_at: "2026-06-10T12:30:00Z"
blocked_by:
  - 2
  - "@7"
  - "acme/api@42"
gh_issue:
  - "42"
area:
  - "backend"
tags:
  - "security"
  - "auth"
---

## Why

So users can authenticate.
"#;

    #[test]
    fn round_trip_full_task() {
        let task = parse_task(FULL_TASK, "0001-auth-middleware.md").unwrap();
        let serialized = serialize_task(&task);
        let reparsed = parse_task(&serialized, "0001-auth-middleware.md").unwrap();

        let fm = &reparsed.frontmatter;
        assert_eq!(fm.id, "0001");
        assert_eq!(fm.title, "Auth middleware");
        assert_eq!(fm.created_at.as_deref(), Some("2026-06-10T11:00:00Z"));
        assert_eq!(fm.started_at.as_deref(), Some("2026-06-10T12:00:00Z"));
        assert_eq!(fm.completed_at.as_deref(), Some("2026-06-10T12:30:00Z"));
        assert_eq!(
            fm.blocked_by,
            vec![
                BlockedByRef::LocalTask("0002".to_owned()),
                BlockedByRef::LocalIssue(7),
                BlockedByRef::ExternalIssue {
                    repo: "acme/api".to_owned(),
                    number: 42
                },
            ]
        );
        assert_eq!(fm.gh_issue, vec!["42"]);
        assert_eq!(fm.area, vec!["backend"]);
        assert_eq!(fm.tags, vec!["security", "auth"]);
        assert!(reparsed.body.contains("## Why"));
    }

    #[test]
    fn round_trip_all_blocker_variants() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nblocked_by:\n  - 2\n  - \"@5\"\n  - \"../plexi:0003\"\n  - \"../plexi@9\"\n  - \"owner/repo:0004\"\n  - \"owner/repo@10\"\n  - \"waiting for fix\"\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        let serialized = serialize_task(&task);
        let reparsed = parse_task(&serialized, "0001-t.md").unwrap();
        assert_eq!(task.frontmatter.blocked_by, reparsed.frontmatter.blocked_by);
    }

    #[test]
    fn round_trip_task_with_priority() {
        let content =
            "---\nid: \"0003\"\ntitle: \"Prioritized\"\nstatus: todo\npriority: p1\n---\n";
        let task = parse_task(content, "0003-prioritized.md").unwrap();
        assert_eq!(task.frontmatter.priority, Some(crate::schema::Priority::P1));
        let serialized = serialize_task(&task);
        let reparsed = parse_task(&serialized, "0003-prioritized.md").unwrap();
        assert_eq!(
            reparsed.frontmatter.priority,
            Some(crate::schema::Priority::P1)
        );
    }

    #[test]
    fn round_trip_task_without_priority() {
        let content = "---\nid: \"0004\"\ntitle: \"NoPrio\"\nstatus: backlog\n---\n";
        let task = parse_task(content, "0004-noprio.md").unwrap();
        assert_eq!(task.frontmatter.priority, None);
        let serialized = serialize_task(&task);
        assert!(!serialized.contains("priority:"));
        let reparsed = parse_task(&serialized, "0004-noprio.md").unwrap();
        assert_eq!(reparsed.frontmatter.priority, None);
    }

    #[test]
    fn round_trip_task_with_size() {
        let content = "---\nid: \"0003\"\ntitle: \"Sized\"\nstatus: todo\nsize: m\n---\n";
        let task = parse_task(content, "0003-sized.md").unwrap();
        assert_eq!(task.frontmatter.size, Some(crate::schema::Size::M));
        let serialized = serialize_task(&task);
        let reparsed = parse_task(&serialized, "0003-sized.md").unwrap();
        assert_eq!(reparsed.frontmatter.size, Some(crate::schema::Size::M));
    }

    #[test]
    fn round_trip_task_without_size() {
        let content = "---\nid: \"0004\"\ntitle: \"NoSize\"\nstatus: backlog\n---\n";
        let task = parse_task(content, "0004-nosize.md").unwrap();
        assert_eq!(task.frontmatter.size, None);
        let serialized = serialize_task(&task);
        assert!(!serialized.contains("size:"));
        let reparsed = parse_task(&serialized, "0004-nosize.md").unwrap();
        assert_eq!(reparsed.frontmatter.size, None);
    }

    #[test]
    fn round_trip_minimal_task() {
        let content = "---\nid: \"0002\"\ntitle: \"Minimal\"\nstatus: backlog\n---\n";
        let task = parse_task(content, "0002-minimal.md").unwrap();
        let serialized = serialize_task(&task);
        let reparsed = parse_task(&serialized, "0002-minimal.md").unwrap();
        assert_eq!(reparsed.frontmatter.id, "0002");
        assert_eq!(reparsed.frontmatter.title, "Minimal");
        assert!(reparsed.frontmatter.estimate.is_none());
        assert!(reparsed.frontmatter.blocked_by.is_empty());
    }

    #[test]
    fn sprint_round_trip() {
        let content =
            "# Sprint 12 \u{00B7} Jun 9\u{2013}20 \u{00B7} goal: ship TUI\n\n- 0001\n- 0002\n";
        let sprint = parse_sprint(content).unwrap();
        let serialized = serialize_sprint(&sprint);
        let reparsed = parse_sprint(&serialized).unwrap();
        assert_eq!(reparsed.header.id, "s12");
        assert_eq!(reparsed.task_ids, vec!["0001", "0002"]);
    }
}
