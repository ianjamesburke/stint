/// Core domain types for tasks, sprints, and blockers.
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

impl TaskStatus {
    /// Parse from the canonical string representation used in frontmatter.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "backlog" => Some(TaskStatus::Backlog),
            "todo" => Some(TaskStatus::Todo),
            "in-progress" => Some(TaskStatus::InProgress),
            "done" => Some(TaskStatus::Done),
            "archived" => Some(TaskStatus::Archived),
            _ => None,
        }
    }

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

/// Parsed `owner/repo#N` GitHub issue reference used in `blocked_by_gh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerRef {
    /// Repository owner (user or org).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Issue or PR number.
    pub number: u64,
}

impl BlockerRef {
    /// Attempt to parse a `owner/repo#N` string.
    pub fn parse(s: &str) -> Option<Self> {
        // Expected: "owner/repo#123"
        let (repo_part, number_str) = s.split_once('#')?;
        let (owner, repo) = repo_part.split_once('/')?;
        let number: u64 = number_str.parse().ok()?;
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        Some(BlockerRef {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number,
        })
    }
}

impl std::fmt::Display for BlockerRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}#{}", self.owner, self.repo, self.number)
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
    /// Sprint this task belongs to (e.g. "s12").
    pub sprint: Option<String>,
    /// Local task IDs that must complete before this one.
    pub blocked_by: Vec<String>,
    /// Cross-repo GitHub issue blockers in `owner/repo#N` format.
    pub blocked_by_gh: Vec<String>,
    /// Free-text description of any additional blockers.
    pub blocked_by_note: Option<String>,
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
            let status = TaskStatus::from_str(s).unwrap();
            assert_eq!(status.as_str(), *s);
        }
    }

    #[test]
    fn task_status_invalid() {
        assert!(TaskStatus::from_str("unknown").is_none());
    }

    #[test]
    fn blocker_ref_parse_valid() {
        let r = BlockerRef::parse("owner/repo#123").unwrap();
        assert_eq!(r.owner, "owner");
        assert_eq!(r.repo, "repo");
        assert_eq!(r.number, 123);
    }

    #[test]
    fn blocker_ref_parse_invalid() {
        assert!(BlockerRef::parse("owner/repo").is_none());
        assert!(BlockerRef::parse("owner#123").is_none());
        assert!(BlockerRef::parse("/repo#123").is_none());
        assert!(BlockerRef::parse("owner/repo#abc").is_none());
    }

    #[test]
    fn blocker_ref_display() {
        let r = BlockerRef {
            owner: "acme".to_owned(),
            repo: "widget".to_owned(),
            number: 42,
        };
        assert_eq!(r.to_string(), "acme/widget#42");
    }
}
