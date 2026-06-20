/// Validate a full task graph against all `stint check` rules.
///
/// All violations are collected — the check does not short-circuit on the
/// first error.  Pass in every `Task` and `Sprint` that exists in the repo.
use std::collections::{HashMap, HashSet};

use chrono::DateTime;
use thiserror::Error;

use crate::schema::{BlockedByRef, Sprint, Task, TaskStatus};
use crate::sprint::numeric_prefix;
use crate::state::done_ids;

/// A single validation violation found by `check`.
#[derive(Debug, Error, PartialEq)]
pub enum CheckError {
    // Rule 1 — required fields
    /// A required field is absent or empty on a task.
    #[error("task {task_id}: missing required field '{field}'")]
    MissingRequiredField {
        /// Task ID (or filename if ID unavailable).
        task_id: String,
        /// Field name.
        field: &'static str,
    },

    // Rule 2 — status enum (already enforced at parse time)
    /// Status value is not a recognised enum variant.
    #[error("task {task_id}: invalid status value '{value}'")]
    InvalidStatus {
        /// Task ID.
        task_id: String,
        /// The bad value.
        value: String,
    },

    // Rule 3 — duration strings (also enforced at parse time)
    /// An estimate or actual field is not a valid duration string.
    #[error("task {task_id}: field '{field}' is not a valid duration")]
    InvalidDuration {
        /// Task ID.
        task_id: String,
        /// Field name (`estimate` or `actual`).
        field: &'static str,
    },

    /// A timestamp field is not valid RFC3339.
    #[error("task {task_id}: field '{field}' is not a valid RFC3339 timestamp")]
    InvalidTimestamp {
        /// Task ID.
        task_id: String,
        /// Field name.
        field: &'static str,
    },

    // Rule 4 — blocked_by local resolution
    /// A `LocalTask` blocker does not match any known task ID.
    #[error("task {task_id}: blocked_by references unknown local task '{ref_id}'")]
    UnresolvedBlockedBy {
        /// Task ID.
        task_id: String,
        /// The unresolved reference.
        ref_id: String,
    },

    // Rule 5 — blocked_by external format
    /// An external blocker ref has an invalid format.
    #[error("task {task_id}: blocked_by entry {entry:?} has invalid format")]
    InvalidBlockedByRef {
        /// Task ID.
        task_id: String,
        /// The malformed entry.
        entry: String,
    },

    // Rule 7 — sprint task references
    /// A sprint index file lists a task ID that does not exist.
    #[error("sprint {sprint_id}: references unknown task '{task_entry}'")]
    SprintUnresolvedTask {
        /// Sprint ID.
        sprint_id: String,
        /// The unresolved entry from the sprint file.
        task_entry: String,
    },

    // Rule 8 — circular blocked_by
    /// A cycle was detected in the local `blocked_by` dependency graph.
    #[error("task {task_id}: circular blocked_by dependency involving '{cycle_member}'")]
    CircularBlockedBy {
        /// Starting task ID.
        task_id: String,
        /// One task in the cycle.
        cycle_member: String,
    },

    // Rule 9 — id matches filename
    /// The task's `id` field does not match the numeric prefix of its filename.
    #[error(
        "task {task_id}: id field {id_field:?} does not match filename prefix {filename_prefix:?}"
    )]
    IdFilenameMismatch {
        /// Task ID as stored on the struct.
        task_id: String,
        /// The `id` field value.
        id_field: String,
        /// The numeric prefix extracted from the filename.
        filename_prefix: String,
    },

    // Rule 10 — duplicate IDs
    /// Two tasks share the same `id` field value.
    #[error("duplicate task id {id:?} found in {file_a:?} and {file_b:?}")]
    DuplicateId {
        /// The duplicated ID.
        id: String,
        /// First filename.
        file_a: String,
        /// Second filename.
        file_b: String,
    },

    // Rule 11 — task state machine
    /// A task is `in-progress` or `done` while still having unresolved
    /// local-task blockers. Only `backlog`/`todo` tasks may carry active
    /// blockers; starting or completing a blocked task is contradictory.
    #[error(
        "task {task_id}: status '{status}' but blocked by unresolved task(s) {blockers:?}; \
         only backlog/todo tasks may have active blockers"
    )]
    BlockedTaskNotPending {
        /// Task ID.
        task_id: String,
        /// The offending status (`in-progress` or `done`).
        status: String,
        /// The unresolved local-task blocker IDs.
        blockers: Vec<String>,
    },

}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate a task graph and return all violations found.
///
/// `tasks` is the full list of parsed tasks; `sprints` is the full list of
/// parsed sprint index files.  Returns an empty `Vec` when everything is valid.
pub fn check(tasks: &[Task], sprints: &[Sprint]) -> Vec<CheckError> {
    let mut errors: Vec<CheckError> = Vec::new();

    let task_id_to_file: HashMap<&str, &str> = tasks
        .iter()
        .map(|t| (t.frontmatter.id.as_str(), t.filename.as_str()))
        .collect();

    let known_task_ids: HashSet<&str> = task_id_to_file.keys().copied().collect();
    let done = done_ids(tasks);

    // Rule 10 — duplicate IDs
    check_duplicate_ids(tasks, &mut errors);

    for task in tasks {
        let id = task.frontmatter.id.as_str();

        // Rule 1 — required fields
        if task.frontmatter.id.is_empty() {
            errors.push(CheckError::MissingRequiredField {
                task_id: task.filename.clone(),
                field: "id",
            });
        }
        if task.frontmatter.title.is_empty() {
            errors.push(CheckError::MissingRequiredField {
                task_id: id.to_owned(),
                field: "title",
            });
        }

        // Rule 9 — id matches filename prefix
        check_id_filename_match(task, &mut errors);

        // Rule 12 — timestamp fields are RFC3339 when present
        check_timestamp(id, "created_at", &task.frontmatter.created_at, &mut errors);
        check_timestamp(id, "started_at", &task.frontmatter.started_at, &mut errors);
        check_timestamp(
            id,
            "completed_at",
            &task.frontmatter.completed_at,
            &mut errors,
        );

        // Rule 4 — LocalTask blockers must resolve to known task IDs
        // Rule 5 — external refs must be structurally valid
        for r in task.frontmatter.blocked_by.iter() {
            validate_blocker_ref(id, r, &known_task_ids, &mut errors);
        }

        // Rule 11 — task state machine: in-progress/done tasks may not carry
        // unresolved local-task blockers.
        if matches!(
            task.frontmatter.status,
            TaskStatus::InProgress | TaskStatus::Done
        ) {
            let unresolved: Vec<String> = task
                .frontmatter
                .blocked_by
                .iter()
                .filter_map(|r| match r {
                    BlockedByRef::LocalTask(ref_id) if !done.contains(ref_id.as_str()) => {
                        Some(ref_id.clone())
                    }
                    _ => None,
                })
                .collect();
            if !unresolved.is_empty() {
                errors.push(CheckError::BlockedTaskNotPending {
                    task_id: id.to_owned(),
                    status: task.frontmatter.status.to_string(),
                    blockers: unresolved,
                });
            }
        }
    }

    // Rule 7 — sprint task references
    for sprint in sprints {
        for entry in &sprint.task_ids {
            let prefix = numeric_prefix(entry);
            if !known_task_ids.contains(prefix) {
                errors.push(CheckError::SprintUnresolvedTask {
                    sprint_id: sprint.header.id.clone(),
                    task_entry: entry.clone(),
                });
            }
        }
    }

    // Rule 8 — circular blocked_by (local task refs only)
    check_cycles(tasks, &mut errors);

    errors
}

fn check_timestamp(
    task_id: &str,
    field: &'static str,
    value: &Option<String>,
    errors: &mut Vec<CheckError>,
) {
    if let Some(value) = value {
        if DateTime::parse_from_rfc3339(value).is_err() {
            errors.push(CheckError::InvalidTimestamp {
                task_id: task_id.to_owned(),
                field,
            });
        }
    }
}

fn validate_blocker_ref(
    owner_id: &str,
    r: &BlockedByRef,
    known_task_ids: &HashSet<&str>,
    errors: &mut Vec<CheckError>,
) {
    match r {
        BlockedByRef::LocalTask(ref_id) => {
            if !known_task_ids.contains(ref_id.as_str()) {
                errors.push(CheckError::UnresolvedBlockedBy {
                    task_id: owner_id.to_owned(),
                    ref_id: ref_id.clone(),
                });
            }
        }
        BlockedByRef::ExternalTask {
            repo,
            task_id: ref_task_id,
        } => {
            if !is_valid_gh_repo(repo) || ref_task_id.is_empty() {
                errors.push(CheckError::InvalidBlockedByRef {
                    task_id: owner_id.to_owned(),
                    entry: r.to_string(),
                });
            }
        }
        BlockedByRef::ExternalIssue { repo, .. } => {
            if !is_valid_gh_repo(repo) {
                errors.push(CheckError::InvalidBlockedByRef {
                    task_id: owner_id.to_owned(),
                    entry: r.to_string(),
                });
            }
        }
        BlockedByRef::LocalDirTask {
            path,
            task_id: ref_task_id,
        } => {
            if path.is_empty() || ref_task_id.is_empty() {
                errors.push(CheckError::InvalidBlockedByRef {
                    task_id: owner_id.to_owned(),
                    entry: r.to_string(),
                });
            }
        }
        BlockedByRef::LocalDirIssue { path, .. } => {
            if path.is_empty() {
                errors.push(CheckError::InvalidBlockedByRef {
                    task_id: owner_id.to_owned(),
                    entry: r.to_string(),
                });
            }
        }
        BlockedByRef::LocalIssue(_) | BlockedByRef::Note(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Rule implementations
// ---------------------------------------------------------------------------

fn is_valid_gh_repo(repo: &str) -> bool {
    // Must match owner/name with non-empty parts and no extra slashes.
    let parts: Vec<&str> = repo.splitn(3, '/').collect();
    parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty()
}

fn check_duplicate_ids(tasks: &[Task], errors: &mut Vec<CheckError>) {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for task in tasks {
        let id = task.frontmatter.id.as_str();
        if let Some(first_file) = seen.get(id) {
            errors.push(CheckError::DuplicateId {
                id: id.to_owned(),
                file_a: first_file.to_string(),
                file_b: task.filename.clone(),
            });
        } else {
            seen.insert(id, task.filename.as_str());
        }
    }
}

fn check_id_filename_match(task: &Task, errors: &mut Vec<CheckError>) {
    let filename_prefix = numeric_prefix(task.filename.trim_end_matches(".md"));
    let id_field = task.frontmatter.id.as_str();
    if id_field != filename_prefix {
        errors.push(CheckError::IdFilenameMismatch {
            task_id: id_field.to_owned(),
            id_field: id_field.to_owned(),
            filename_prefix: filename_prefix.to_owned(),
        });
    }
}

/// Rule 8 — detect cycles in the local `blocked_by` graph (DFS colouring).
fn check_cycles(tasks: &[Task], errors: &mut Vec<CheckError>) {
    // Only traverse LocalTask refs — external refs can't form local cycles.
    let id_to_local_blockers: HashMap<&str, Vec<String>> = tasks
        .iter()
        .map(|t| {
            let local: Vec<String> = t
                .frontmatter
                .blocked_by
                .iter()
                .filter_map(|r| {
                    if let BlockedByRef::LocalTask(id) = r {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();
            (t.frontmatter.id.as_str(), local)
        })
        .collect();

    let mut color: HashMap<&str, u8> = tasks
        .iter()
        .map(|t| (t.frontmatter.id.as_str(), 0u8))
        .collect();

    for task in tasks {
        if *color.get(task.frontmatter.id.as_str()).unwrap_or(&0) == 0 {
            let mut stack: Vec<(&str, bool)> = vec![(task.frontmatter.id.as_str(), false)];
            while let Some((node, leaving)) = stack.last().copied() {
                if leaving {
                    stack.pop();
                    color.insert(node, 2);
                    continue;
                }
                if let Some(top) = stack.last_mut() {
                    *top = (node, true);
                }
                color.insert(node, 1);

                if let Some(blockers) = id_to_local_blockers.get(node) {
                    for dep in blockers {
                        match color.get(dep.as_str()).copied().unwrap_or(0) {
                            1 => {
                                errors.push(CheckError::CircularBlockedBy {
                                    task_id: node.to_owned(),
                                    cycle_member: dep.clone(),
                                });
                            }
                            0 => {
                                stack.push((dep.as_str(), false));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_task;
    use crate::sprint::parse_sprint;

    fn make_task(id: &str, title: &str, extra: &str) -> Task {
        let filename = format!("{}-slug.md", id);
        let content = format!(
            "---\nid: \"{}\"\ntitle: \"{}\"\nstatus: backlog\n{}\n---\n",
            id, title, extra
        );
        parse_task(&content, &filename).unwrap()
    }

    fn make_sprint(number: u32, task_ids: &[&str]) -> Sprint {
        let entries: String = task_ids.iter().map(|id| format!("- {}\n", id)).collect();
        let content = format!("# Sprint {} · Jan 1–15 · goal: test\n\n{}", number, entries);
        parse_sprint(&content).unwrap()
    }

    #[test]
    fn empty_id_is_missing_required_field() {
        use crate::schema::{TaskFrontmatter, TaskStatus};
        let task = Task {
            frontmatter: TaskFrontmatter {
                id: String::new(),
                title: "Some title".to_owned(),
                status: TaskStatus::Backlog,
                priority: None,
                estimate: None,
                actual: None,
                created_at: None,
                started_at: None,
                completed_at: None,
                blocked_by: vec![],
                gh_issue: vec![],
                area: vec![],
                tags: vec![],
            },
            body: String::new(),
            filename: "0001-slug.md".to_owned(),
        };
        let errors = check(&[task], &[]);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::MissingRequiredField { field: "id", .. })));
    }

    #[test]
    fn empty_title_is_missing_required_field() {
        use crate::schema::{TaskFrontmatter, TaskStatus};
        let task = Task {
            frontmatter: TaskFrontmatter {
                id: "0001".to_owned(),
                title: String::new(),
                status: TaskStatus::Backlog,
                priority: None,
                estimate: None,
                actual: None,
                created_at: None,
                started_at: None,
                completed_at: None,
                blocked_by: vec![],
                gh_issue: vec![],
                area: vec![],
                tags: vec![],
            },
            body: String::new(),
            filename: "0001-slug.md".to_owned(),
        };
        let errors = check(&[task], &[]);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::MissingRequiredField { field: "title", .. })));
    }

    #[test]
    fn invalid_timestamp_fields_are_reported() {
        let task = parse_task(
            "---\nid: \"0001\"\ntitle: \"Task\"\nstatus: backlog\ncreated_at: nope\n---\n",
            "0001-task.md",
        )
        .unwrap();
        let errors = check(&[task], &[]);
        assert!(errors.iter().any(|e| {
            matches!(
                e,
                CheckError::InvalidTimestamp {
                    task_id,
                    field: "created_at"
                } if task_id == "0001"
            )
        }));
    }

    #[test]
    fn clean_graph_produces_no_errors() {
        let tasks = vec![
            make_task("0001", "Task A", ""),
            make_task("0002", "Task B", "blocked_by: [\"0001\"]"),
        ];
        assert!(check(&tasks, &[]).is_empty());
    }

    #[test]
    fn unresolved_blocked_by() {
        let tasks = vec![make_task("0001", "Task A", "blocked_by: [\"9999\"]")];
        let errors = check(&tasks, &[]);
        assert!(errors.iter().any(
            |e| matches!(e, CheckError::UnresolvedBlockedBy { ref_id, .. } if ref_id == "9999")
        ));
    }

    #[test]
    fn external_task_ref_does_not_require_local_resolution() {
        let tasks = vec![make_task(
            "0001",
            "Task A",
            "blocked_by: [\"owner/repo:0999\"]",
        )];
        let errors = check(&tasks, &[]);
        assert!(errors
            .iter()
            .all(|e| !matches!(e, CheckError::UnresolvedBlockedBy { .. })));
    }

    #[test]
    fn external_issue_ref_passes_format_check() {
        let tasks = vec![make_task("0001", "Task A", "blocked_by: [\"acme/api@7\"]")];
        let errors = check(&tasks, &[]);
        assert!(errors
            .iter()
            .all(|e| !matches!(e, CheckError::InvalidBlockedByRef { .. })));
    }

    #[test]
    fn note_blocker_passes_check() {
        let tasks = vec![make_task(
            "0001",
            "Task A",
            "blocked_by: [\"waiting for upstream\"]",
        )];
        let errors = check(&tasks, &[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn local_dir_task_passes_format_check() {
        let tasks = vec![make_task(
            "0001",
            "Task A",
            "blocked_by: [\"../plexi:0146\"]",
        )];
        let errors = check(&tasks, &[]);
        assert!(errors
            .iter()
            .all(|e| !matches!(e, CheckError::InvalidBlockedByRef { .. })));
    }

    #[test]
    fn sprint_unresolved_task() {
        let tasks = vec![make_task("0001", "Task A", "")];
        let sprints = vec![make_sprint(1, &["9999"])];
        let errors = check(&tasks, &sprints);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::SprintUnresolvedTask { .. })));
    }

    #[test]
    fn sprint_resolved_task_with_slug() {
        let tasks = vec![make_task("0001", "Task A", "")];
        let sprints = vec![make_sprint(1, &["0001-slug"])];
        let errors = check(&tasks, &sprints);
        assert!(errors
            .iter()
            .all(|e| !matches!(e, CheckError::SprintUnresolvedTask { .. })));
    }

    #[test]
    fn circular_blocked_by() {
        let t1 = make_task("0001", "A", "blocked_by: [\"0002\"]");
        let t2 = make_task("0002", "B", "blocked_by: [\"0001\"]");
        let errors = check(&[t1, t2], &[]);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::CircularBlockedBy { .. })));
    }

    #[test]
    fn self_referencing_blocked_by() {
        let task = make_task("0001", "A", "blocked_by: [\"0001\"]");
        let errors = check(&[task], &[]);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::CircularBlockedBy { .. })));
    }

    #[test]
    fn id_filename_mismatch() {
        let content = "---\nid: \"0099\"\ntitle: \"T\"\nstatus: backlog\n---\n";
        let task = parse_task(content, "0001-slug.md").unwrap();
        let errors = check(&[task], &[]);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::IdFilenameMismatch { .. })));
    }

    #[test]
    fn duplicate_ids() {
        let t1 = make_task("0001", "A", "");
        let content = "---\nid: \"0001\"\ntitle: \"B\"\nstatus: backlog\n---\n";
        let t2 = parse_task(content, "0001-other.md").unwrap();
        let errors = check(&[t1, t2], &[]);
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::DuplicateId { id, .. } if id == "0001")));
    }

    #[test]
    fn in_progress_with_unresolved_blocker_is_error() {
        let blocker = make_task("0001", "Blocker", "");
        let content =
            "---\nid: \"0002\"\ntitle: \"B\"\nstatus: in-progress\nblocked_by: [\"0001\"]\n---\n";
        let dependent = parse_task(content, "0002-slug.md").unwrap();
        let errors = check(&[blocker, dependent], &[]);
        assert!(errors.iter().any(|e| matches!(
            e,
            CheckError::BlockedTaskNotPending { task_id, blockers, .. }
                if task_id == "0002" && blockers == &vec!["0001".to_owned()]
        )));
    }

    #[test]
    fn in_progress_with_done_blocker_is_ok() {
        let content_blocker = "---\nid: \"0001\"\ntitle: \"A\"\nstatus: done\n---\n";
        let blocker = parse_task(content_blocker, "0001-slug.md").unwrap();
        let content =
            "---\nid: \"0002\"\ntitle: \"B\"\nstatus: in-progress\nblocked_by: [\"0001\"]\n---\n";
        let dependent = parse_task(content, "0002-slug.md").unwrap();
        let errors = check(&[blocker, dependent], &[]);
        assert!(errors
            .iter()
            .all(|e| !matches!(e, CheckError::BlockedTaskNotPending { .. })));
    }

    #[test]
    fn todo_with_unresolved_blocker_is_not_state_error() {
        let blocker = make_task("0001", "A", "");
        let content =
            "---\nid: \"0002\"\ntitle: \"B\"\nstatus: todo\nblocked_by: [\"0001\"]\n---\n";
        let dependent = parse_task(content, "0002-slug.md").unwrap();
        let errors = check(&[blocker, dependent], &[]);
        assert!(errors
            .iter()
            .all(|e| !matches!(e, CheckError::BlockedTaskNotPending { .. })));
    }

    #[test]
    fn multiple_violations_all_collected() {
        let tasks = vec![make_task(
            "0001",
            "A",
            "blocked_by: [\"9999\", \"another/task:9998\"]",
        )];
        let errors = check(&tasks, &[]);
        // At least the unresolved local task error
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::UnresolvedBlockedBy { .. })));
    }

}
