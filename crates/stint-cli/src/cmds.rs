/// All command implementations.  Each function takes a `&StintRepo` and
/// whatever arguments the command needs.  No `std::process::exit` here —
/// callers handle exit codes.
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context};
use stint_core::check::check;
use stint_core::duration::Duration;
use stint_core::mutate::{
    add_actual, new_sprint_content, new_task_content, next_task_id, resolve_id, set_status,
    title_to_slug,
};
use stint_core::schema::TaskStatus;
use stint_core::serialize::serialize_task;
use stint_core::sprint::{numeric_prefix, sprint_add_task, sprint_remove_task};
use stint_core::status::compute_status;

use crate::repo::StintRepo;

// ---------------------------------------------------------------------------
// Task commands
// ---------------------------------------------------------------------------

/// Create a new task file, open `$EDITOR`, print the created path.
pub fn cmd_add(repo: &StintRepo, title: &str) -> anyhow::Result<PathBuf> {
    repo.ensure_dirs()?;
    let tasks = repo.load_tasks()?;
    let id = next_task_id(&tasks);
    let slug = title_to_slug(title);
    let filename = if slug.is_empty() {
        format!("{}.md", id)
    } else {
        format!("{}-{}.md", id, slug)
    };
    let path = repo.tasks_dir().join(&filename);
    let content = new_task_content(&id, title);
    repo.write_task(&path, &content)?;
    open_editor(&path)?;
    Ok(path)
}

/// List tasks, optionally filtered.
pub fn cmd_list(
    repo: &StintRepo,
    status_filter: Option<&str>,
    sprint_filter: Option<&str>,
    area_filter: Option<&str>,
    tag_filter: Option<&str>,
) -> anyhow::Result<Vec<TaskRow>> {
    let tasks = repo.load_tasks()?;

    let rows = tasks
        .iter()
        .filter(|t| {
            if let Some(s) = status_filter {
                if t.frontmatter.status.as_str() != s {
                    return false;
                }
            }
            if let Some(sp) = sprint_filter {
                match &t.frontmatter.sprint {
                    Some(ts) if ts == sp => {}
                    _ => return false,
                }
            }
            if let Some(a) = area_filter {
                if !t.frontmatter.area.iter().any(|x| x == a) {
                    return false;
                }
            }
            if let Some(tag) = tag_filter {
                if !t.frontmatter.tags.iter().any(|x| x == tag) {
                    return false;
                }
            }
            true
        })
        .map(|t| TaskRow {
            id: t.frontmatter.id.clone(),
            title: t.frontmatter.title.clone(),
            status: t.frontmatter.status.as_str().to_owned(),
            estimate: t.frontmatter.estimate.map(|d| d.to_string()),
            sprint: t.frontmatter.sprint.clone(),
        })
        .collect();
    Ok(rows)
}

/// Print a `cmd_list` result to stdout at 80 columns.
pub fn print_list(rows: &[TaskRow]) {
    if rows.is_empty() {
        println!("(no tasks)");
        return;
    }
    println!("{:<6} {:<11} {:<8} {:<6} {}", "ID", "STATUS", "ESTIMATE", "SPRINT", "TITLE");
    println!("{}", "-".repeat(78));
    for row in rows {
        println!(
            "{:<6} {:<11} {:<8} {:<6} {}",
            row.id,
            row.status,
            row.estimate.as_deref().unwrap_or("-"),
            row.sprint.as_deref().unwrap_or("-"),
            truncate(&row.title, 40),
        );
    }
}

/// A summarised task row for `stint list` output.
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub estimate: Option<String>,
    pub sprint: Option<String>,
}

/// Print a full task (frontmatter table + body) to stdout.
pub fn cmd_show(repo: &StintRepo, id_input: &str) -> anyhow::Result<()> {
    let path = repo.resolve_task_path(id_input)?;
    let task = repo.read_task(&path)?;
    let fm = &task.frontmatter;

    println!("ID:          {}", fm.id);
    println!("Title:       {}", fm.title);
    println!("Status:      {}", fm.status);
    if let Some(e) = &fm.estimate {
        println!("Estimate:    {}", e);
    }
    if let Some(a) = &fm.actual {
        println!("Actual:      {}", a);
    }
    if let Some(s) = &fm.sprint {
        println!("Sprint:      {}", s);
    }
    if !fm.blocked_by.is_empty() {
        println!("Blocked by:  {}", fm.blocked_by.join(", "));
    }
    if !fm.blocked_by_gh.is_empty() {
        println!("Blocked (GH):{}", fm.blocked_by_gh.join(", "));
    }
    if let Some(note) = &fm.blocked_by_note {
        println!("Blocked note:{}", note);
    }
    if !fm.area.is_empty() {
        println!("Area:        {}", fm.area.join(", "));
    }
    if !fm.tags.is_empty() {
        println!("Tags:        {}", fm.tags.join(", "));
    }
    if !task.body.is_empty() {
        println!();
        print!("{}", task.body);
    }
    Ok(())
}

/// Open a task file in `$EDITOR`.
pub fn cmd_edit(repo: &StintRepo, id_input: &str) -> anyhow::Result<PathBuf> {
    let path = repo.resolve_task_path(id_input)?;
    open_editor(&path)?;
    Ok(path)
}

/// Set a task's status to `done`.  If the task has no `actual` time set,
/// prompt on stdin; pass `actual_override` to skip the prompt (e.g. in tests).
pub fn cmd_done(
    repo: &StintRepo,
    id_input: &str,
    actual_override: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let path = repo.resolve_task_path(id_input)?;
    let mut task = repo.read_task(&path)?;

    set_status(&mut task, TaskStatus::Done);

    if task.frontmatter.actual.is_none() {
        let duration_str = match actual_override {
            Some(s) => s.to_owned(),
            None => {
                let mut buf = String::new();
                eprint!("Actual time spent (e.g. 2h, 30m) [skip]: ");
                io::stderr().flush().ok();
                io::stdin().read_line(&mut buf).context("read stdin")?;
                buf.trim().to_owned()
            }
        };
        if !duration_str.is_empty() {
            let d: Duration = duration_str
                .parse()
                .with_context(|| format!("invalid duration {:?}", duration_str))?;
            add_actual(&mut task, d);
        }
    }

    let content = serialize_task(&task);
    repo.write_task(&path, &content)?;
    Ok(path)
}

/// Add `duration` to a task's `actual` field.
pub fn cmd_log(repo: &StintRepo, id_input: &str, duration_str: &str) -> anyhow::Result<PathBuf> {
    let d: Duration = duration_str
        .parse()
        .with_context(|| format!("invalid duration {:?}", duration_str))?;
    let path = repo.resolve_task_path(id_input)?;
    let mut task = repo.read_task(&path)?;
    add_actual(&mut task, d);
    let content = serialize_task(&task);
    repo.write_task(&path, &content)?;
    Ok(path)
}

/// Set a task's status to `archived`.
pub fn cmd_archive(repo: &StintRepo, id_input: &str) -> anyhow::Result<PathBuf> {
    let path = repo.resolve_task_path(id_input)?;
    let mut task = repo.read_task(&path)?;
    set_status(&mut task, TaskStatus::Archived);
    let content = serialize_task(&task);
    repo.write_task(&path, &content)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Sprint commands
// ---------------------------------------------------------------------------

/// Create a new sprint file.
pub fn cmd_sprint_new(
    repo: &StintRepo,
    id_input: &str,
    date_range: &str,
    goal: Option<&str>,
) -> anyhow::Result<PathBuf> {
    repo.ensure_dirs()?;
    let id = crate::repo::normalize_sprint_id(id_input);
    let path = repo.sprints_dir().join(format!("{}.md", id));
    if path.exists() {
        bail!("sprint {:?} already exists at {}", id, path.display());
    }
    let content = new_sprint_content(&id, date_range, goal);
    repo.write_sprint(&path, &content)?;
    Ok(path)
}

/// List all sprints.
pub fn cmd_sprint_list(repo: &StintRepo) -> anyhow::Result<()> {
    let sprints = repo.load_sprints()?;
    if sprints.is_empty() {
        println!("(no sprints)");
        return Ok(());
    }
    println!("{:<6} {:<20} {}", "ID", "DATE RANGE", "TASKS");
    println!("{}", "-".repeat(50));
    for s in &sprints {
        println!(
            "{:<6} {:<20} {}",
            s.header.id,
            s.header.date_range,
            s.task_ids.len()
        );
    }
    Ok(())
}

/// Show a sprint with task summaries.
pub fn cmd_sprint_show(repo: &StintRepo, id_input: &str) -> anyhow::Result<()> {
    let path = repo.resolve_sprint_path(id_input)?;
    let tasks = repo.load_tasks()?;
    let content = repo.read_sprint_raw(&path)?;
    let sprint = stint_core::sprint::parse_sprint(&content)
        .with_context(|| format!("parse {}", path.display()))?;

    println!("Sprint: {}", sprint.header.id);
    println!("Range:  {}", sprint.header.date_range);
    if let Some(g) = &sprint.header.goal {
        println!("Goal:   {}", g);
    }
    println!();
    if sprint.task_ids.is_empty() {
        println!("(no tasks)");
        return Ok(());
    }
    println!("{:<6} {:<11} {:<8} {}", "ID", "STATUS", "ESTIMATE", "TITLE");
    println!("{}", "-".repeat(78));
    for entry in &sprint.task_ids {
        let prefix = numeric_prefix(entry);
        match tasks.iter().find(|t| t.frontmatter.id == prefix) {
            Some(t) => println!(
                "{:<6} {:<11} {:<8} {}",
                t.frontmatter.id,
                t.frontmatter.status,
                t.frontmatter
                    .estimate
                    .map(|d| d.to_string())
                    .as_deref()
                    .unwrap_or("-"),
                truncate(&t.frontmatter.title, 50),
            ),
            None => println!("{:<6} (not found)", entry),
        }
    }
    Ok(())
}

/// Append a task to a sprint file.
pub fn cmd_sprint_add(
    repo: &StintRepo,
    sprint_id: &str,
    task_id: &str,
) -> anyhow::Result<PathBuf> {
    let task_id = resolve_id(task_id);
    let path = repo.resolve_sprint_path(sprint_id)?;
    let content = repo.read_sprint_raw(&path)?;
    let updated = sprint_add_task(&content, &task_id);
    repo.write_sprint(&path, &updated)?;
    Ok(path)
}

/// Open a sprint file in `$EDITOR` for manual reordering.
///
/// The sprint file is an ordered list of task IDs — the user moves lines to
/// change priority order.  After the editor exits, the file is re-parsed to
/// ensure it is still valid.
pub fn cmd_sprint_reorder(repo: &StintRepo, id_input: &str) -> anyhow::Result<PathBuf> {
    let path = repo.resolve_sprint_path(id_input)?;
    open_editor(&path)?;
    // Validate the file is still parseable after manual edits.
    let content = repo.read_sprint_raw(&path)?;
    stint_core::sprint::parse_sprint(&content)
        .with_context(|| format!("sprint file is no longer valid after reorder: {}", path.display()))?;
    Ok(path)
}

/// Remove a task from a sprint file.
pub fn cmd_sprint_remove(
    repo: &StintRepo,
    sprint_id: &str,
    task_id: &str,
) -> anyhow::Result<PathBuf> {
    let task_id = resolve_id(task_id);
    let path = repo.resolve_sprint_path(sprint_id)?;
    let content = repo.read_sprint_raw(&path)?;
    let updated = sprint_remove_task(&content, &task_id);
    repo.write_sprint(&path, &updated)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Validation & status
// ---------------------------------------------------------------------------

/// Run `stint check` and return all error strings.  Empty = valid.
///
/// `cross_repo`: when true, cross-repo reference resolution would walk sibling
/// repositories to validate external task IDs.  This is a stub — the feature
/// is out of scope for v1.  The flag is accepted and acknowledged so the CLI
/// surface matches the spec, but only the local check runs.
pub fn cmd_check(repo: &StintRepo, cross_repo: bool) -> anyhow::Result<Vec<String>> {
    if cross_repo {
        // STUB: cross-repo resolution not yet implemented.
        eprintln!("cross-repo resolution not yet implemented");
    }
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let errors = check(&tasks, &sprints);
    Ok(errors.iter().map(|e| e.to_string()).collect())
}

/// Print the `stint status` summary.
pub fn cmd_status(repo: &StintRepo) -> anyhow::Result<()> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let report = compute_status(&tasks, &sprints, None);

    println!("Open tasks: {}", report.open_count);

    if report.blocked_tasks.is_empty() {
        println!("Blocked:    none");
    } else {
        println!("Blocked ({}):", report.blocked_tasks.len());
        for bt in &report.blocked_tasks {
            let mut reasons = Vec::new();
            if !bt.blocked_by.is_empty() {
                reasons.push(format!("tasks: {}", bt.blocked_by.join(", ")));
            }
            if !bt.blocked_by_gh.is_empty() {
                reasons.push(format!("gh: {}", bt.blocked_by_gh.join(", ")));
            }
            if let Some(note) = &bt.blocked_by_note {
                reasons.push(format!("note: {}", note));
            }
            println!("  {} {} — {}", bt.id, bt.title, reasons.join("; "));
        }
    }

    if let Some(p) = &report.sprint_progress {
        println!();
        println!("Sprint {}:", p.sprint_id);
        println!(
            "  Tasks:     {}/{} done",
            p.done_count, p.task_count
        );
        println!(
            "  Committed: {}h",
            minutes_to_hours_str(p.committed_minutes)
        );
        println!(
            "  Logged:    {}h",
            minutes_to_hours_str(p.logged_minutes)
        );
        println!(
            "  Remaining: {}h",
            minutes_to_hours_str(p.remaining_minutes)
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Open `path` in `$EDITOR`.  If `$EDITOR` is not set, falls back to `vi`.
///
/// In test environments, if `STINT_TEST_EDITOR` is set to `none`, this is a
/// no-op (so tests can call `cmd_add` without needing a real editor).
pub fn open_editor(path: &std::path::Path) -> anyhow::Result<()> {
    // Allow tests to suppress editor invocation.
    if std::env::var("STINT_TEST_EDITOR").as_deref() == Ok("none") {
        return Ok(());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_owned());
    let status = Command::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("launch editor {:?}", editor))?;
    if !status.success() {
        bail!("editor exited with status {}", status);
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

fn minutes_to_hours_str(minutes: u32) -> String {
    let h = minutes / 60;
    let m = minutes % 60;
    if m == 0 {
        format!("{}", h)
    } else {
        format!("{}.{}", h, (m * 10 / 60))
    }
}
