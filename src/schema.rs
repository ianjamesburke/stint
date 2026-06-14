/// Core domain types for tasks, sprints, and blockers.
use std::str::FromStr;

use crate::duration::Duration;

/// Status lifecycle for a task.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    /// Not yet scheduled.
    Backlog,
    /// Scheduled but not started.
    Todo,
    /// Currently being worked on.
    InProgress,
    /// Completed.
    Done,
    /// Removed from active consideration.
    Archived,
}

/// Error returned when a `TaskStatus` string is not recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatusParseError(pub String);

impl std::fmt::Display for TaskStatusParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown status {:?}; expected one of backlog, todo, in-progress, done, archived",
            self.0
        )
    }
}

impl std::error::Error for TaskStatusParseError {}

impl FromStr for TaskStatus {
    type Err = TaskStatusParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "backlog" => Ok(TaskStatus::Backlog),
            "todo" => Ok(TaskStatus::Todo),
            "in-progress" => Ok(TaskStatus::InProgress),
            "done" => Ok(TaskStatus::Done),
            "archived" => Ok(TaskStatus::Archived),
            _ => Err(TaskStatusParseError(s.to_owned())),
        }
    }
}

impl TaskStatus {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in-progress",
            TaskStatus::Done => "done",
            TaskStatus::Archived => "archived",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A unified blocker reference. One field, many types, inferred by syntax.
///
/// Syntax rules (in parse order):
/// - YAML integer or all-digit string → `LocalTask` (zero-padded to 4 digits)
/// - `@N`                             → `LocalIssue`
/// - `../path:NNNN` or `./path:NNNN` → `LocalDirTask`
/// - `../path@N`   or `./path@N`     → `LocalDirIssue`
/// - `owner/repo:NNNN`               → `ExternalTask`
/// - `owner/repo@N`                  → `ExternalIssue`
/// - anything else                   → `Note` (free-text)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedByRef {
    /// Local stint task (zero-padded 4-digit ID).
    LocalTask(String),
    /// Local GitHub issue number.
    LocalIssue(u64),
    /// Stint task in a sibling directory.
    LocalDirTask { path: String, task_id: String },
    /// GitHub issue resolved via a sibling directory.
    LocalDirIssue { path: String, number: u64 },
    /// Stint task in an external GitHub repo (`owner/repo`).
    ExternalTask { repo: String, task_id: String },
    /// GitHub issue in an external repo.
    ExternalIssue { repo: String, number: u64 },
    /// Free-text blocker note.
    Note(String),
}

impl BlockedByRef {
    /// Parse from a YAML string value.
    pub fn from_str(s: &str) -> Self {
        // All-digits: local task
        if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
            let n: u64 = s.parse().unwrap_or(0);
            return BlockedByRef::LocalTask(format!("{:0>4}", n));
        }

        // @N: local issue
        if let Some(rest) = s.strip_prefix('@') {
            if let Ok(n) = rest.parse::<u64>() {
                return BlockedByRef::LocalIssue(n);
            }
        }

        // Relative path (starts with ./ or ../)
        if s.starts_with("./") || s.starts_with("../") {
            if let Some((path, task_id)) = s.split_once(':') {
                if !task_id.is_empty() && task_id.chars().all(|c| c.is_ascii_digit()) {
                    let n: u64 = task_id.parse().unwrap_or(0);
                    return BlockedByRef::LocalDirTask {
                        path: path.to_owned(),
                        task_id: format!("{:0>4}", n),
                    };
                }
            }
            if let Some((path, issue_str)) = s.split_once('@') {
                if let Ok(n) = issue_str.parse::<u64>() {
                    return BlockedByRef::LocalDirIssue {
                        path: path.to_owned(),
                        number: n,
                    };
                }
            }
        }

        // owner/repo:task or owner/repo@issue (must contain / not at start)
        if let Some(slash) = s.find('/') {
            if slash > 0 {
                if let Some((repo_part, task_id)) = s.split_once(':') {
                    if repo_part.contains('/')
                        && !task_id.is_empty()
                        && task_id.chars().all(|c| c.is_ascii_digit())
                    {
                        let n: u64 = task_id.parse().unwrap_or(0);
                        return BlockedByRef::ExternalTask {
                            repo: repo_part.to_owned(),
                            task_id: format!("{:0>4}", n),
                        };
                    }
                }
                if let Some((repo_part, issue_str)) = s.split_once('@') {
                    if repo_part.contains('/') {
                        if let Ok(n) = issue_str.parse::<u64>() {
                            return BlockedByRef::ExternalIssue {
                                repo: repo_part.to_owned(),
                                number: n,
                            };
                        }
                    }
                }
            }
        }

        BlockedByRef::Note(s.to_owned())
    }

    /// Parse from a YAML integer value.
    pub fn from_int(n: u64) -> Self {
        BlockedByRef::LocalTask(format!("{:0>4}", n))
    }

    /// The on-disk string representation. `LocalTask` serializes as a bare
    /// integer (no quotes); all other variants serialize as quoted strings.
    pub fn is_integer_form(&self) -> bool {
        matches!(self, BlockedByRef::LocalTask(_))
    }
}

impl std::fmt::Display for BlockedByRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockedByRef::LocalTask(id) => write!(f, "{}", id),
            BlockedByRef::LocalIssue(n) => write!(f, "@{}", n),
            BlockedByRef::LocalDirTask { path, task_id } => write!(f, "{}:{}", path, task_id),
            BlockedByRef::LocalDirIssue { path, number } => write!(f, "{}@{}", path, number),
            BlockedByRef::ExternalTask { repo, task_id } => write!(f, "{}:{}", repo, task_id),
            BlockedByRef::ExternalIssue { repo, number } => write!(f, "{}@{}", repo, number),
            BlockedByRef::Note(s) => write!(f, "{}", s),
        }
    }
}

/// Parsed and coerced task frontmatter.
#[derive(Debug, Clone)]
pub struct TaskFrontmatter {
    /// Zero-padded 4-digit task ID string (e.g. "0001").
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Time budgeted for this task.
    pub estimate: Option<Duration>,
    /// Time logged so far.
    pub actual: Option<Duration>,
    /// UTC RFC3339 timestamp for when the task was created.
    pub created_at: Option<String>,
    /// UTC RFC3339 timestamp for when work started.
    pub started_at: Option<String>,
    /// UTC RFC3339 timestamp for when work completed.
    pub completed_at: Option<String>,
    /// Unified blocker references (local tasks, issues, external refs, notes).
    pub blocked_by: Vec<BlockedByRef>,
    /// Linked GitHub issue numbers (as strings for uniformity).
    pub gh_issue: Vec<String>,
    /// Area/component labels.
    pub area: Vec<String>,
    /// Arbitrary tags.
    pub tags: Vec<String>,
}

/// A fully parsed task: frontmatter plus freeform markdown body.
#[derive(Debug, Clone)]
pub struct Task {
    /// Parsed frontmatter fields.
    pub frontmatter: TaskFrontmatter,
    /// Everything below the closing `---` delimiter.
    pub body: String,
    /// Original filename (without directory), e.g. `"0001-auth-middleware.md"`.
    pub filename: String,
}

impl Task {
    /// Convenience accessor for the task ID.
    pub fn id(&self) -> &str {
        &self.frontmatter.id
    }
}

/// Parsed sprint file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintHeader {
    /// Sprint identifier, e.g. `"s12"`.
    pub id: String,
    /// Human-readable date range string, e.g. `"Jun 9–20"`.
    pub date_range: String,
    /// Optional sprint goal text.
    pub goal: Option<String>,
}

/// A fully parsed sprint: header plus ordered task ID list.
#[derive(Debug, Clone)]
pub struct Sprint {
    /// Parsed header.
    pub header: SprintHeader,
    /// Task IDs in priority order (may include slug suffix or numeric-only).
    pub task_ids: Vec<String>,
}

impl Sprint {
    /// Convenience accessor for the sprint ID.
    pub fn id(&self) -> &str {
        &self.header.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_round_trip() {
        for s in &["backlog", "todo", "in-progress", "done", "archived"] {
            let status: TaskStatus = s.parse().unwrap();
            assert_eq!(status.as_str(), *s);
        }
    }

    #[test]
    fn task_status_invalid() {
        assert!("unknown".parse::<TaskStatus>().is_err());
    }

    #[test]
    fn blocked_by_ref_local_task_from_int() {
        assert_eq!(
            BlockedByRef::from_int(146),
            BlockedByRef::LocalTask("0146".to_owned())
        );
        assert_eq!(
            BlockedByRef::from_int(10),
            BlockedByRef::LocalTask("0010".to_owned())
        );
    }

    #[test]
    fn blocked_by_ref_local_task_from_str() {
        assert_eq!(
            BlockedByRef::from_str("0146"),
            BlockedByRef::LocalTask("0146".to_owned())
        );
        assert_eq!(
            BlockedByRef::from_str("146"),
            BlockedByRef::LocalTask("0146".to_owned())
        );
        assert_eq!(
            BlockedByRef::from_str("8"),
            BlockedByRef::LocalTask("0008".to_owned())
        );
    }

    #[test]
    fn blocked_by_ref_local_issue() {
        assert_eq!(
            BlockedByRef::from_str("@123"),
            BlockedByRef::LocalIssue(123)
        );
    }

    #[test]
    fn blocked_by_ref_local_dir_task() {
        assert_eq!(
            BlockedByRef::from_str("../plexi:0146"),
            BlockedByRef::LocalDirTask {
                path: "../plexi".to_owned(),
                task_id: "0146".to_owned()
            }
        );
        assert_eq!(
            BlockedByRef::from_str("../plexi:146"),
            BlockedByRef::LocalDirTask {
                path: "../plexi".to_owned(),
                task_id: "0146".to_owned()
            }
        );
    }

    #[test]
    fn blocked_by_ref_local_dir_issue() {
        assert_eq!(
            BlockedByRef::from_str("../plexi@99"),
            BlockedByRef::LocalDirIssue {
                path: "../plexi".to_owned(),
                number: 99
            }
        );
    }

    #[test]
    fn blocked_by_ref_external_task() {
        assert_eq!(
            BlockedByRef::from_str("ianjamesburke/PLEXI:0146"),
            BlockedByRef::ExternalTask {
                repo: "ianjamesburke/PLEXI".to_owned(),
                task_id: "0146".to_owned()
            }
        );
    }

    #[test]
    fn blocked_by_ref_external_issue() {
        assert_eq!(
            BlockedByRef::from_str("acme/api@7"),
            BlockedByRef::ExternalIssue {
                repo: "acme/api".to_owned(),
                number: 7
            }
        );
    }

    #[test]
    fn blocked_by_ref_note() {
        assert_eq!(
            BlockedByRef::from_str("waiting for upstream fix"),
            BlockedByRef::Note("waiting for upstream fix".to_owned())
        );
        assert_eq!(
            BlockedByRef::from_str("blocked on design review"),
            BlockedByRef::Note("blocked on design review".to_owned())
        );
    }

    #[test]
    fn blocked_by_ref_display_round_trips() {
        let cases = [
            BlockedByRef::LocalTask("0146".to_owned()),
            BlockedByRef::LocalIssue(7),
            BlockedByRef::LocalDirTask {
                path: "../plexi".to_owned(),
                task_id: "0146".to_owned(),
            },
            BlockedByRef::LocalDirIssue {
                path: "../plexi".to_owned(),
                number: 99,
            },
            BlockedByRef::ExternalTask {
                repo: "acme/api".to_owned(),
                task_id: "0012".to_owned(),
            },
            BlockedByRef::ExternalIssue {
                repo: "acme/api".to_owned(),
                number: 7,
            },
            BlockedByRef::Note("waiting for fix".to_owned()),
        ];
        for r in &cases {
            // Display → from_str should round-trip (except LocalTask int form)
            if !matches!(r, BlockedByRef::LocalTask(_)) {
                assert_eq!(BlockedByRef::from_str(&r.to_string()), *r);
            }
        }
    }
}
