/// Parse task markdown files (frontmatter + body) into typed `Task` structs.
use serde::Deserialize;
use thiserror::Error;

use crate::duration::Duration;
use crate::schema::{Task, TaskFrontmatter, TaskStatus};

/// Errors that can occur while parsing a task file.
#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    /// The file has no opening `---` frontmatter delimiter.
    #[error("missing frontmatter opening delimiter '---'")]
    MissingFrontmatter,

    /// YAML deserialization failed.
    #[error("yaml parse error: {0}")]
    Yaml(String),

    /// A required field is absent or empty.
    #[error("required field missing: {0}")]
    MissingField(&'static str),

    /// A field value is not valid.
    #[error("invalid field '{field}': {reason}")]
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Human-readable explanation.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Raw serde types — only used during deserialization
// ---------------------------------------------------------------------------

/// Accepts either a YAML scalar or a YAML sequence during deserialization.
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        OneOrMany::Many(vec![])
    }
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            OneOrMany::One(v) => vec![v],
            OneOrMany::Many(v) => v,
        }
    }
}

/// Raw frontmatter as returned by serde_yaml before type coercion.
#[derive(Deserialize, Debug)]
struct RawFrontmatter {
    id: Option<String>,
    title: Option<String>,
    status: Option<String>,
    estimate: Option<String>,
    actual: Option<String>,
    sprint: Option<String>,
    #[serde(default)]
    blocked_by: Option<OneOrMany<String>>,
    #[serde(default)]
    blocked_by_gh: Option<OneOrMany<String>>,
    blocked_by_note: Option<String>,
    #[serde(default)]
    gh_issue: Option<serde_yaml::Value>,
    #[serde(default)]
    area: Option<OneOrMany<String>>,
    #[serde(default)]
    tags: Option<OneOrMany<String>>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a task file from its string content and filename.
///
/// `filename` should be the bare filename (e.g. `"0001-auth-middleware.md"`).
/// It is stored verbatim on the returned `Task` and used by `check()`.
pub fn parse_task(content: &str, filename: &str) -> Result<Task, ParseError> {
    let (frontmatter_str, body) = split_frontmatter(content)?;

    let raw: RawFrontmatter =
        serde_yaml::from_str(frontmatter_str).map_err(|e| ParseError::Yaml(e.to_string()))?;

    let id = raw
        .id
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingField("id"))?;

    let title = raw
        .title
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingField("title"))?;

    let status_str = raw
        .status
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingField("status"))?;

    let status = status_str
        .parse::<TaskStatus>()
        .map_err(|e| ParseError::InvalidField {
            field: "status",
            reason: e.to_string(),
        })?;

    let estimate = raw
        .estimate
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<Duration>().map_err(|e| ParseError::InvalidField {
                field: "estimate",
                reason: e.to_string(),
            })
        })
        .transpose()?;

    let actual = raw
        .actual
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<Duration>().map_err(|e| ParseError::InvalidField {
                field: "actual",
                reason: e.to_string(),
            })
        })
        .transpose()?;

    let blocked_by = raw.blocked_by.map(|x| x.into_vec()).unwrap_or_default();

    let blocked_by_gh = raw.blocked_by_gh.map(|x| x.into_vec()).unwrap_or_default();

    let gh_issue = raw
        .gh_issue
        .map(yaml_values_to_strings)
        .transpose()?
        .unwrap_or_default();

    let area = raw.area.map(|x| x.into_vec()).unwrap_or_default();
    let tags = raw.tags.map(|x| x.into_vec()).unwrap_or_default();

    let frontmatter = TaskFrontmatter {
        id,
        title,
        status,
        estimate,
        actual,
        sprint: raw.sprint.filter(|s| !s.is_empty()),
        blocked_by,
        blocked_by_gh,
        blocked_by_note: raw.blocked_by_note.filter(|s| !s.is_empty()),
        gh_issue,
        area,
        tags,
    };

    Ok(Task {
        frontmatter,
        body: body.to_owned(),
        filename: filename.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Split `content` into `(frontmatter_str, body_str)`.
///
/// Expects the content to begin with a `---` line, followed by YAML, followed
/// by another `---` line.  Everything after the closing delimiter is the body.
fn split_frontmatter(content: &str) -> Result<(&str, &str), ParseError> {
    // The file must start with "---" (possibly with leading whitespace/BOM stripped).
    let content = content.trim_start_matches('\u{feff}'); // strip BOM if present

    let after_open = content
        .strip_prefix("---")
        .ok_or(ParseError::MissingFrontmatter)?;

    // The opening --- must be followed immediately by a newline (or end of file).
    let after_newline = after_open
        .strip_prefix('\n')
        .or_else(|| after_open.strip_prefix("\r\n"))
        .ok_or(ParseError::MissingFrontmatter)?;

    // Find the closing --- on its own line (must be followed by \n or end of string).
    let close_pos = after_newline
        .find("\n---\n")
        .or_else(|| {
            if after_newline.ends_with("\n---") {
                Some(after_newline.len() - 4)
            } else {
                None
            }
        })
        .ok_or(ParseError::MissingFrontmatter)?;

    let frontmatter_str = &after_newline[..close_pos];
    let after_close = &after_newline[close_pos + 4..]; // skip "\n---"

    // Body is everything after the optional newline following the close marker.
    let body = after_close
        .strip_prefix('\n')
        .or_else(|| after_close.strip_prefix("\r\n"))
        .unwrap_or(after_close);

    Ok((frontmatter_str, body))
}

/// Convert a serde_yaml Value to a Vec<String> for the gh_issue field.
///
/// Handles scalars (int or string) and sequences thereof.
fn yaml_values_to_strings(v: serde_yaml::Value) -> Result<Vec<String>, ParseError> {
    match v {
        serde_yaml::Value::Number(n) => Ok(vec![n.to_string()]),
        serde_yaml::Value::String(s) => Ok(vec![s]),
        serde_yaml::Value::Sequence(items) => {
            items.into_iter().map(yaml_scalar_to_string).collect()
        }
        serde_yaml::Value::Null => Ok(vec![]),
        other => Err(ParseError::InvalidField {
            field: "gh_issue",
            reason: format!("expected number, string, or list; got {:?}", other),
        }),
    }
}

/// Convert a single scalar serde_yaml Value to a String.
fn yaml_scalar_to_string(v: serde_yaml::Value) -> Result<String, ParseError> {
    match v {
        serde_yaml::Value::Number(n) => Ok(n.to_string()),
        serde_yaml::Value::String(s) => Ok(s),
        other => Err(ParseError::InvalidField {
            field: "gh_issue",
            reason: format!("expected number or string, got {:?}", other),
        }),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_TASK: &str = r#"---
id: "0001"
title: "Auth middleware"
status: in-progress
estimate: "4h"
actual: "30m"
sprint: "s12"
blocked_by:
  - "0002"
blocked_by_gh: "acme/api#7"
blocked_by_note: "waiting for upstream fix"
gh_issue: 42
area:
  - backend
tags:
  - security
  - auth
---

## Why

So users can authenticate.
"#;

    #[test]
    fn parse_full_task() {
        let task = parse_task(FULL_TASK, "0001-auth-middleware.md").unwrap();
        let fm = &task.frontmatter;
        assert_eq!(fm.id, "0001");
        assert_eq!(fm.title, "Auth middleware");
        assert_eq!(fm.status, TaskStatus::InProgress);
        assert_eq!(fm.estimate, Some(Duration::from_minutes(240)));
        assert_eq!(fm.actual, Some(Duration::from_minutes(30)));
        assert_eq!(fm.sprint.as_deref(), Some("s12"));
        assert_eq!(fm.blocked_by, vec!["0002"]);
        assert_eq!(fm.blocked_by_gh, vec!["acme/api#7"]);
        assert_eq!(
            fm.blocked_by_note.as_deref(),
            Some("waiting for upstream fix")
        );
        assert_eq!(fm.gh_issue, vec!["42"]);
        assert_eq!(fm.area, vec!["backend"]);
        assert_eq!(fm.tags, vec!["security", "auth"]);
        assert!(task.body.contains("## Why"));
    }

    #[test]
    fn coerce_scalar_blocked_by() {
        let content =
            "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nblocked_by: \"0002\"\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert_eq!(task.frontmatter.blocked_by, vec!["0002"]);
    }

    #[test]
    fn coerce_scalar_tags() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\ntags: security\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert_eq!(task.frontmatter.tags, vec!["security"]);
    }

    #[test]
    fn coerce_gh_issue_integer() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\ngh_issue: 99\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert_eq!(task.frontmatter.gh_issue, vec!["99"]);
    }

    #[test]
    fn coerce_gh_issue_list_of_ints() {
        let content =
            "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\ngh_issue: [1, 2, 3]\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert_eq!(task.frontmatter.gh_issue, vec!["1", "2", "3"]);
    }

    #[test]
    fn empty_optional_lists() {
        let content =
            "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nblocked_by: []\ntags: []\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert!(task.frontmatter.blocked_by.is_empty());
        assert!(task.frontmatter.tags.is_empty());
    }

    #[test]
    fn null_optional_lists() {
        let content =
            "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nblocked_by: ~\ntags: ~\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert!(task.frontmatter.blocked_by.is_empty());
        assert!(task.frontmatter.tags.is_empty());
    }

    #[test]
    fn missing_required_field_id() {
        let content = "---\ntitle: \"T\"\nstatus: backlog\n---\n";
        assert!(matches!(
            parse_task(content, "0001-t.md"),
            Err(ParseError::MissingField("id"))
        ));
    }

    #[test]
    fn missing_required_field_status() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\n---\n";
        assert!(matches!(
            parse_task(content, "0001-t.md"),
            Err(ParseError::MissingField("status"))
        ));
    }

    #[test]
    fn invalid_status() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: flying\n---\n";
        assert!(matches!(
            parse_task(content, "0001-t.md"),
            Err(ParseError::InvalidField {
                field: "status",
                ..
            })
        ));
    }

    #[test]
    fn invalid_estimate_duration() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nestimate: \"4x\"\n---\n";
        assert!(matches!(
            parse_task(content, "0001-t.md"),
            Err(ParseError::InvalidField {
                field: "estimate",
                ..
            })
        ));
    }

    #[test]
    fn no_frontmatter() {
        let content = "Just a body with no frontmatter.";
        assert!(matches!(
            parse_task(content, "0001-t.md"),
            Err(ParseError::MissingFrontmatter)
        ));
    }

    #[test]
    fn body_is_preserved() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\n---\n\nBody text here.\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert_eq!(task.body.trim(), "Body text here.");
    }

    #[test]
    fn empty_body() {
        let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\n---\n";
        let task = parse_task(content, "0001-t.md").unwrap();
        assert_eq!(task.body, "");
    }
}
