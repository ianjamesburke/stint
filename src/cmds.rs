/// All command implementations.  Each function takes a `&StintRepo` and
/// whatever arguments the command needs.  No `std::process::exit` here —
/// callers handle exit codes.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use owo_colors::OwoColorize;

use anyhow::{bail, Context};
use chrono::{DateTime, SecondsFormat, Utc};
use stint::check::check;
use stint::duration::Duration;
use stint::mutate::{
    add_actual, new_sprint_content, new_task_content, next_task_id, resolve_id, restart_task,
    set_actual, set_completed_at, set_started_at_if_absent, set_status, title_to_slug,
};
use stint::next::{compute_next, compute_next_with_resolution, NextOptions, NextReport, NextTask};
use stint::schema::{BlockedByRef, Sprint, TaskStatus};
use stint::serialize::serialize_task;
use stint::sprint::{
    normalize_sprint_id, numeric_prefix, sprint_add_task, sprint_remove_task, task_link,
};
use stint::state::{
    active_blockers, active_blockers_with_resolution, classify_with_resolution, done_ids,
    is_blocked_with_resolution,
};
use stint::status::compute_status_with_resolution;

use stint::repo::StintRepo;

// ---------------------------------------------------------------------------
// Init command
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    pub repo: PathBuf,
    pub imported: usize,
    pub skipped: usize,
}

/// Create `.stint/` and optionally import open GitHub issues.
pub fn cmd_init(
    root: &Path,
    force: bool,
    with_github: bool,
    github_repo: Option<&str>,
) -> anyhow::Result<InitReport> {
    let repo = StintRepo::init(root, force)?;
    let mut report = InitReport {
        repo: repo.stint_dir.clone(),
        imported: 0,
        skipped: 0,
    };

    if with_github {
        let issues = fetch_open_github_issues(github_repo)?;
        let imported = import_github_issues(&repo, &issues)?;
        report.imported = imported.imported;
        report.skipped = imported.skipped;
    }

    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubImportReport {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubIssue {
    number: String,
    title: String,
    body: String,
    labels: Vec<String>,
}

fn fetch_open_github_issues(repo: Option<&str>) -> anyhow::Result<Vec<GithubIssue>> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "issue",
        "list",
        "--state",
        "open",
        "--limit",
        "1000",
        "--json",
        "number,title,body,labels",
    ]);
    if let Some(repo) = repo {
        cmd.args(["--repo", repo]);
    }

    let output = cmd.output().context("run gh issue list")?;
    if !output.status.success() {
        bail!(
            "gh issue list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    parse_github_issues_json(&output.stdout)
}

pub(crate) fn parse_github_issues_json(bytes: &[u8]) -> anyhow::Result<Vec<GithubIssue>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).context("parse gh issue JSON")?;
    let issues = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("expected gh issue list JSON array"))?;

    let mut parsed = Vec::with_capacity(issues.len());
    for issue in issues {
        let number = issue
            .get("number")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("GitHub issue missing numeric number"))?
            .to_string();
        let title = issue
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("GitHub issue {} missing title", number))?
            .to_owned();
        let body = issue
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let labels = issue
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|label| label.get("name").and_then(|name| name.as_str()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        parsed.push(GithubIssue {
            number,
            title,
            body,
            labels,
        });
    }

    Ok(parsed)
}

pub(crate) fn import_github_issues(
    repo: &StintRepo,
    issues: &[GithubIssue],
) -> anyhow::Result<GithubImportReport> {
    repo.ensure_dirs()?;
    let mut tasks = repo.load_tasks()?;
    let mut seen_issues: HashSet<String> = tasks
        .iter()
        .flat_map(|task| task.frontmatter.gh_issue.iter().cloned())
        .collect();

    let mut imported = 0usize;
    let mut skipped = 0usize;
    for issue in issues {
        if seen_issues.contains(&issue.number) {
            skipped += 1;
            continue;
        }

        let id = next_task_id(&tasks);
        let filename = format!("{}-{}.md", id, title_to_slug(&issue.title));
        let path = repo.tasks_dir().join(filename);
        let content = github_issue_task_content(&id, issue)?;
        repo.write_task(&path, &content)?;
        let task = repo.read_task(&path)?;
        tasks.push(task);
        seen_issues.insert(issue.number.clone());
        imported += 1;
    }

    Ok(GithubImportReport { imported, skipped })
}

fn github_issue_task_content(id: &str, issue: &GithubIssue) -> anyhow::Result<String> {
    let created_at = timestamp_or_now(None)?;
    let mut task = new_task_content(id, &issue.title, Some(&created_at));
    let mut imported_fields = string_list_yaml("gh_issue", std::slice::from_ref(&issue.number));
    if !issue.labels.is_empty() {
        imported_fields.push_str(&string_list_yaml("tags", &issue.labels));
    }
    task = task.replace(
        "---\n\n## Why",
        &format!("{}---\n\n## Why", imported_fields),
    );

    if !issue.body.trim().is_empty() {
        task.push_str("\n## GitHub Issue\n\n");
        task.push_str(issue.body.trim());
        task.push('\n');
    }

    Ok(task)
}

fn string_list_yaml(key: &str, values: &[String]) -> String {
    let mut out = format!("{}:\n", key);
    for value in values {
        out.push_str(&format!("  - \"{}\"\n", yaml_escape(value)));
    }
    out
}

fn yaml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Task commands
// ---------------------------------------------------------------------------

/// Create a new task file, open `$EDITOR`, print the created path.
pub fn cmd_add(
    repo: &StintRepo,
    title: &str,
    priority: Option<&str>,
    size: Option<&str>,
) -> anyhow::Result<PathBuf> {
    // Validate priority early.
    let parsed_priority = priority
        .map(|p| {
            p.parse::<stint::schema::Priority>()
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .transpose()?;

    // Validate size early.
    let parsed_size = size
        .map(|s| {
            s.parse::<stint::schema::Size>()
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .transpose()?;

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
    let created_at = timestamp_or_now(None)?;
    let mut content = new_task_content(&id, title, Some(&created_at));

    // Insert priority line after status line if provided.
    if let Some(prio) = parsed_priority {
        content = content.replacen(
            "status: backlog\n",
            &format!("status: backlog\npriority: {}\n", prio),
            1,
        );
    }

    // Insert size line after the priority line (or status line) if provided.
    if let Some(sz) = parsed_size {
        let (anchor, replacement) = match parsed_priority {
            Some(prio) => (
                format!("priority: {}\n", prio),
                format!("priority: {}\nsize: {}\n", prio, sz),
            ),
            None => (
                "status: backlog\n".to_owned(),
                format!("status: backlog\nsize: {}\n", sz),
            ),
        };
        content = content.replacen(&anchor, &replacement, 1);
    }

    repo.write_task(&path, &content)?;
    open_editor(&path)?;
    Ok(path)
}

/// List tasks, optionally filtered.
pub fn cmd_list(
    repo: &StintRepo,
    status_filter: Option<&str>,
    include_all: bool,
    blocked_filter: Option<bool>,
    sprint_filter: Option<&str>,
    area_filter: Option<&str>,
    tag_filter: Option<&str>,
    priority_filter: Option<&str>,
) -> anyhow::Result<Vec<TaskRow>> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let done = done_ids(&tasks);
    let direct_resolution = repo.resolve_direct_task_path_blockers(&tasks)?;
    let blocker_resolution = &direct_resolution.blocker_resolution;

    // Build a task_id -> sprint_id lookup from the sprint index files.
    let task_sprint_map = task_to_sprint_map(&sprints);

    let rows = stint::filter::filter_tasks(
        &tasks,
        &sprints,
        status_filter,
        sprint_filter,
        area_filter,
        tag_filter,
        priority_filter,
    )
    .into_iter()
    .filter(|t| {
        status_filter.is_some()
            || include_all
            || !matches!(
                t.frontmatter.status,
                TaskStatus::Done | TaskStatus::Archived
            )
    })
    .filter(|t| match blocked_filter {
        Some(blocked) => is_blocked_with_resolution(t, &done, blocker_resolution) == blocked,
        None => true,
    })
    .map(|t| TaskRow {
        id: t.frontmatter.id.clone(),
        title: t.frontmatter.title.clone(),
        state: classify_with_resolution(t, &done, blocker_resolution)
            .as_str()
            .to_owned(),
        blocked: is_blocked_with_resolution(t, &done, blocker_resolution),
        blockers: active_blockers_with_resolution(t, &done, blocker_resolution),
        estimate: t.frontmatter.estimate.map(|d| d.to_string()),
        sprint: task_sprint_map.get(t.frontmatter.id.as_str()).cloned(),
        priority: t.frontmatter.priority.map(|p| p.to_string()),
        size: t.frontmatter.size.map(|s| s.to_string()),
    })
    .collect();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Terminal styling. Color is applied only on an interactive stdout with NO_COLOR
// unset; piped/redirected output stays plain so it parses cleanly.
// ---------------------------------------------------------------------------

fn color_on() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Pad `state` to `width`, then color it by lifecycle.
fn paint_state(state: &str, width: usize, on: bool) -> String {
    let field = format!("{:<width$}", state);
    if !on {
        return field;
    }
    match state {
        "ready" => field.green().to_string(),
        "active" => field.cyan().bold().to_string(),
        "blocked" => field.yellow().to_string(),
        "done" | "archived" => field.bright_black().to_string(),
        "backlog" => field.dimmed().to_string(),
        _ => field,
    }
}

fn dim(s: &str, on: bool) -> String {
    if on {
        s.dimmed().to_string()
    } else {
        s.to_owned()
    }
}

fn bold(s: &str, on: bool) -> String {
    if on {
        s.bold().to_string()
    } else {
        s.to_owned()
    }
}

fn paint_id(s: &str, on: bool) -> String {
    if on {
        s.cyan().bold().to_string()
    } else {
        s.to_owned()
    }
}

/// Print a `cmd_list` result to stdout at 80 columns.
pub fn print_list(rows: &[TaskRow]) {
    if rows.is_empty() {
        println!("(no tasks)");
        return;
    }
    let on = color_on();
    let header = format!(
        "{:<6} {:<9} {:<4} {:<4} {:<8} {:<6} {}",
        "ID", "STATE", "PRI", "SIZE", "ESTIMATE", "SPRINT", "TITLE"
    );
    println!("{}", dim(&header, on));
    for row in rows {
        println!(
            "{} {} {:<4} {:<4} {:<8} {:<6} {}",
            paint_id(&format!("{:<6}", row.id), on),
            paint_state(&row.state, 9, on),
            row.priority.as_deref().unwrap_or("-"),
            row.size.as_deref().unwrap_or("-"),
            row.estimate.as_deref().unwrap_or("-"),
            row.sprint.as_deref().unwrap_or("-"),
            truncate(&row.title, 38),
        );
        if row.blocked && !row.blockers.is_empty() {
            println!(
                "       {}",
                dim(
                    &format!("blocked by {}", format_blockers_inline(&row.blockers)),
                    on
                )
            );
        }
    }
}

/// A summarised task row for `stint list` output.
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub state: String,
    pub blocked: bool,
    pub blockers: Vec<BlockedByRef>,
    pub estimate: Option<String>,
    pub sprint: Option<String>,
    pub priority: Option<String>,
    pub size: Option<String>,
}

/// Print a full task (frontmatter table + body) to stdout.
pub fn cmd_show(repo: &StintRepo, id_input: &str) -> anyhow::Result<()> {
    let path = repo.resolve_task_path(id_input)?;
    let task = repo.read_task(&path)?;
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let done = done_ids(&tasks);
    let direct_resolution = repo.resolve_direct_task_path_blockers(&tasks)?;
    let active =
        active_blockers_with_resolution(&task, &done, &direct_resolution.blocker_resolution);
    let fm = &task.frontmatter;

    // Look up sprint membership from the index files.
    let sprint_id = task_to_sprint_map(&sprints).get(fm.id.as_str()).cloned();

    println!("ID:          {}", fm.id);
    println!("Title:       {}", fm.title);
    println!("Status:      {}", fm.status);
    if let Some(p) = &fm.priority {
        println!("Priority:    {}", p);
    }
    if let Some(s) = &fm.size {
        println!("Size:        {}", s);
    }
    if let Some(e) = &fm.estimate {
        println!("Estimate:    {}", e);
    }
    if let Some(a) = &fm.actual {
        println!("Actual:      {}", a);
    }
    if let Some(started_at) = &fm.started_at {
        println!("Started at:  {}", started_at);
    }
    if let Some(completed_at) = &fm.completed_at {
        println!("Completed at:{}", completed_at);
    }
    if let Some(s) = &sprint_id {
        println!("Sprint:      {}", s);
    }
    if !active.is_empty() {
        println!("Blocked by:  {}", format_blockers_inline(&active));
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

/// Remove one or more task files.
pub fn cmd_remove(repo: &StintRepo, id_inputs: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    if id_inputs.is_empty() {
        bail!("at least one task id is required");
    }

    let mut paths = Vec::with_capacity(id_inputs.len());
    for id in id_inputs {
        paths.push(repo.resolve_task_path(id)?);
    }

    for path in &paths {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }

    Ok(paths)
}

/// Set a task's status to `in-progress` and record `started_at`.
pub fn cmd_start(
    repo: &StintRepo,
    id_input: &str,
    restart: bool,
    started_at_override: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let path = repo.resolve_task_path(id_input)?;
    let mut task = repo.read_task(&path)?;
    let started_at = timestamp_or_now(started_at_override)?;

    set_status(&mut task, TaskStatus::InProgress);
    if restart {
        restart_task(&mut task, started_at);
    } else if task.frontmatter.started_at.is_some() {
        bail!(
            "task {} already has started_at; use --restart to replace it",
            task.id()
        );
    } else {
        set_started_at_if_absent(&mut task, started_at);
    }

    let content = serialize_task(&task);
    repo.write_task(&path, &content)?;
    Ok(path)
}

/// Set a task's status to `done`.  If the task has no `actual` time set,
/// prompt on stdin; pass `actual_override` to skip the prompt (e.g. in tests).
pub fn cmd_done(
    repo: &StintRepo,
    id_input: &str,
    actual_override: Option<&str>,
    started_at_override: Option<&str>,
    completed_at_override: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let path = repo.resolve_task_path(id_input)?;
    let mut task = repo.read_task(&path)?;
    let completed_at = timestamp_or_now(completed_at_override)?;

    set_status(&mut task, TaskStatus::Done);
    set_completed_at(&mut task, completed_at.clone());

    if let Some(duration_str) = actual_override {
        if task.frontmatter.started_at.is_none() {
            if let Some(override_value) = started_at_override {
                task.frontmatter.started_at = Some(normalize_timestamp(override_value)?);
            }
        }
        let d: Duration = duration_str
            .parse()
            .with_context(|| format!("invalid duration {:?}", duration_str))?;
        set_actual(&mut task, d);
    } else if task.frontmatter.actual.is_none() {
        let started_at = if let Some(existing) = task.frontmatter.started_at.clone() {
            Some(existing)
        } else if let Some(override_value) = started_at_override {
            let normalized = normalize_timestamp(override_value)?;
            task.frontmatter.started_at = Some(normalized.clone());
            Some(normalized)
        } else {
            prompt_started_at()?
        };

        if let Some(started_at) = started_at {
            let elapsed = elapsed_duration(&started_at, &completed_at)?;
            set_actual(&mut task, elapsed);
        } else {
            let duration_str = prompt_actual()?;
            if !duration_str.is_empty() {
                let d: Duration = duration_str
                    .parse()
                    .with_context(|| format!("invalid duration {:?}", duration_str))?;
                add_actual(&mut task, d);
            }
        }
    }

    warn_if_variance_large(&task);

    let content = serialize_task(&task);
    repo.write_task(&path, &content)?;
    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnblockedTask {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunedBlockerTask {
    pub id: String,
    pub title: String,
}

pub fn tasks_unblocked_by_done(
    repo: &StintRepo,
    done_id_input: &str,
) -> anyhow::Result<Vec<UnblockedTask>> {
    let done_path = repo.resolve_task_path(done_id_input)?;
    let done_task = repo.read_task(&done_path)?;
    let done_id = done_task.frontmatter.id;
    let tasks = repo.load_tasks()?;
    let done = done_ids(&tasks);

    let mut unblocked = Vec::new();
    for task in &tasks {
        if !matches!(
            task.frontmatter.status,
            TaskStatus::Backlog | TaskStatus::Todo | TaskStatus::InProgress
        ) {
            continue;
        }

        let waits_on_done_task = task
            .frontmatter
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, BlockedByRef::LocalTask(id) if *id == done_id));
        if waits_on_done_task && active_blockers(task, &done).is_empty() {
            unblocked.push(UnblockedTask {
                id: task.frontmatter.id.clone(),
                title: task.frontmatter.title.clone(),
            });
        }
    }

    Ok(unblocked)
}

pub fn prune_done_blocker_refs(
    repo: &StintRepo,
    done_id_input: &str,
) -> anyhow::Result<Vec<PrunedBlockerTask>> {
    let done_path = repo.resolve_task_path(done_id_input)?;
    let done_task = repo.read_task(&done_path)?;
    let done_id = done_task.frontmatter.id;
    let tasks = repo.load_tasks()?;

    let mut pruned = Vec::new();
    for mut task in tasks {
        if !matches!(
            task.frontmatter.status,
            TaskStatus::Backlog | TaskStatus::Todo | TaskStatus::InProgress
        ) {
            continue;
        }

        let before_len = task.frontmatter.blocked_by.len();
        task.frontmatter
            .blocked_by
            .retain(|blocker| !matches!(blocker, BlockedByRef::LocalTask(id) if *id == done_id));

        if task.frontmatter.blocked_by.len() == before_len {
            continue;
        }

        let id = task.frontmatter.id.clone();
        let title = task.frontmatter.title.clone();
        let path = repo.resolve_task_path(&id)?;
        let content = serialize_task(&task);
        repo.write_task(&path, &content)?;
        pruned.push(PrunedBlockerTask { id, title });
    }

    Ok(pruned)
}

fn prompt_actual() -> anyhow::Result<String> {
    let mut buf = String::new();
    eprint!("Actual time spent (e.g. 2h, 30m) [skip]: ");
    io::stderr().flush().ok();
    io::stdin().read_line(&mut buf).context("read stdin")?;
    Ok(buf.trim().to_owned())
}

fn prompt_started_at() -> anyhow::Result<Option<String>> {
    let mut buf = String::new();
    eprint!("Started at (UTC RFC3339, e.g. 2026-06-10T12:00:00Z) [skip]: ");
    io::stderr().flush().ok();
    io::stdin().read_line(&mut buf).context("read stdin")?;
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(normalize_timestamp(trimmed)?))
    }
}

fn timestamp_or_now(override_value: Option<&str>) -> anyhow::Result<String> {
    match override_value {
        Some(value) => normalize_timestamp(value),
        None => Ok(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)),
    }
}

fn normalize_timestamp(value: &str) -> anyhow::Result<String> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid timestamp {:?}", value))?;
    Ok(parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn elapsed_duration(started_at: &str, completed_at: &str) -> anyhow::Result<Duration> {
    let started = DateTime::parse_from_rfc3339(started_at)
        .with_context(|| format!("invalid started_at {:?}", started_at))?;
    let completed = DateTime::parse_from_rfc3339(completed_at)
        .with_context(|| format!("invalid completed_at {:?}", completed_at))?;
    let elapsed = completed.signed_duration_since(started);
    if elapsed.num_seconds() < 0 {
        bail!("completed_at is before started_at");
    }
    let minutes = ((elapsed.num_seconds() + 59) / 60) as u32;
    Ok(Duration::from_minutes(minutes))
}

fn warn_if_variance_large(task: &stint::schema::Task) {
    let Some(estimate) = task.frontmatter.estimate else {
        return;
    };
    let Some(actual) = task.frontmatter.actual else {
        return;
    };
    let estimate_minutes = estimate.minutes();
    let actual_minutes = actual.minutes();
    if estimate_minutes == 0 || actual_minutes == 0 {
        return;
    }
    if actual_minutes > estimate_minutes * 2 || estimate_minutes > actual_minutes * 2 {
        eprintln!(
            "note: actual ({}) differs from estimate ({}) by more than 2x; add a variance note",
            actual, estimate
        );
    }
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

/// Promote backlog tasks to todo (out of icebox). Accepts explicit IDs, or a sprint/tag selector.
pub fn cmd_ready(
    repo: &StintRepo,
    ids: &[String],
    sprint: Option<&str>,
    tag: Option<&str>,
) -> anyhow::Result<Vec<PathBuf>> {
    let candidates = resolve_status_transition_targets(repo, ids, sprint, tag)?;
    let mut promoted = Vec::new();
    for (path, mut task) in candidates {
        match task.frontmatter.status {
            TaskStatus::Backlog => {
                set_status(&mut task, TaskStatus::Todo);
                let content = serialize_task(&task);
                repo.write_task(&path, &content)?;
                promoted.push(path);
            }
            TaskStatus::Todo => {
                // Already in the ready pool — skip silently if selected by sprint/tag,
                // or error if the caller targeted this task explicitly by ID.
                if !ids.is_empty() {
                    anyhow::bail!("task {} is already todo", task.id());
                }
            }
            _ => {
                if !ids.is_empty() {
                    anyhow::bail!(
                        "task {} has status {}; only backlog tasks can be made ready",
                        task.id(),
                        task.frontmatter.status
                    );
                }
            }
        }
    }
    Ok(promoted)
}

/// Send todo tasks back to backlog (icebox). Accepts explicit IDs, or a sprint/tag selector.
pub fn cmd_defer(
    repo: &StintRepo,
    ids: &[String],
    sprint: Option<&str>,
    tag: Option<&str>,
) -> anyhow::Result<Vec<PathBuf>> {
    let candidates = resolve_status_transition_targets(repo, ids, sprint, tag)?;
    let mut deferred = Vec::new();
    for (path, mut task) in candidates {
        match task.frontmatter.status {
            TaskStatus::Todo => {
                set_status(&mut task, TaskStatus::Backlog);
                let content = serialize_task(&task);
                repo.write_task(&path, &content)?;
                deferred.push(path);
            }
            TaskStatus::Backlog => {
                // Already iced — skip silently for bulk selectors, error for explicit IDs.
                if !ids.is_empty() {
                    anyhow::bail!("task {} is already backlog", task.id());
                }
            }
            _ => {
                if !ids.is_empty() {
                    anyhow::bail!(
                        "task {} has status {}; only todo tasks can be deferred",
                        task.id(),
                        task.frontmatter.status
                    );
                }
            }
        }
    }
    Ok(deferred)
}

/// Resolve the set of tasks targeted by a ready/defer call.
///
/// If explicit `ids` are given they take precedence and the sprint/tag selectors are ignored.
fn resolve_status_transition_targets(
    repo: &StintRepo,
    ids: &[String],
    sprint: Option<&str>,
    tag: Option<&str>,
) -> anyhow::Result<Vec<(PathBuf, stint::schema::Task)>> {
    if !ids.is_empty() {
        let mut out = Vec::new();
        for id in ids {
            let path = repo.resolve_task_path(id)?;
            let task = repo.read_task(&path)?;
            out.push((path, task));
        }
        return Ok(out);
    }

    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;

    // Build sprint task ID set if a sprint filter was requested.
    let sprint_task_ids: Option<std::collections::HashSet<String>> = sprint.and_then(|sp| {
        sprints.iter().find(|s| s.header.id == sp).map(|s| {
            s.task_ids
                .iter()
                .map(|e| numeric_prefix(e).to_owned())
                .collect()
        })
    });

    let mut out = Vec::new();
    for task in tasks {
        if let Some(ref ids_set) = sprint_task_ids {
            if !ids_set.contains(task.frontmatter.id.as_str()) {
                continue;
            }
        } else if sprint.is_some() {
            // Sprint was requested but not found — no tasks match.
            continue;
        }
        if let Some(t) = tag {
            if !task.frontmatter.tags.iter().any(|tg| tg == t) {
                continue;
            }
        }
        let path = repo.resolve_task_path(&task.frontmatter.id)?;
        out.push((path, task));
    }
    Ok(out)
}

/// Show the next ready task(s). Pure read -- no mutation.
pub fn cmd_next(
    repo: &StintRepo,
    sprint: Option<&str>,
    include_area_conflicts: bool,
    include_backlog: bool,
) -> anyhow::Result<NextReport> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let direct_resolution = repo.resolve_direct_task_path_blockers(&tasks)?;
    Ok(compute_next_with_resolution(
        &tasks,
        &sprints,
        NextOptions {
            sprint,
            include_area_conflicts,
            include_backlog,
        },
        &direct_resolution.blocker_resolution,
    ))
}

/// Claim a task as in-progress.
///
/// With an explicit `id`, behaves like the old `start` command.
/// Without an `id`, auto-claims the top ready task under an advisory lock so
/// parallel callers each receive a distinct task.
pub fn cmd_claim(
    repo: &StintRepo,
    id: Option<&str>,
    sprint: Option<&str>,
    restart: bool,
    started_at_override: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    if let Some(id) = id {
        let path = cmd_start(repo, id, restart, started_at_override)?;
        if json {
            use serde_json::json;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({ "claimed": id, "path": path.to_string_lossy() })
                )
                .unwrap()
            );
        } else {
            println!("claimed: {}", id);
        }
    } else {
        with_claim_lock(repo, || {
            let tasks = repo.load_tasks()?;
            let sprints = repo.load_sprints()?;
            let report = compute_next(
                &tasks,
                &sprints,
                NextOptions {
                    sprint,
                    include_area_conflicts: false,
                    include_backlog: false,
                },
            );
            match report.ready.first() {
                None => {
                    if json {
                        println!("{{\"claimed\": null}}");
                    } else {
                        println!("nothing claimable");
                    }
                    Ok(())
                }
                Some(next_task) => {
                    let id = next_task.id.clone();
                    let path = cmd_start(repo, &id, restart, started_at_override)?;
                    if json {
                        use serde_json::json;
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &json!({ "claimed": id, "path": path.to_string_lossy() })
                            )
                            .unwrap()
                        );
                    } else {
                        println!("claimed: {}", id);
                        println!("  {}", next_task.title);
                    }
                    Ok(())
                }
            }
        })?;
    }
    Ok(())
}

/// Acquire `.stint/claim.lock` (mkdir-based advisory lock), run `f`, then
/// release. Retries up to 20 times at 50 ms intervals before giving up.
fn with_claim_lock<T, F: FnOnce() -> anyhow::Result<T>>(
    repo: &StintRepo,
    f: F,
) -> anyhow::Result<T> {
    let lock = repo.stint_dir.join("claim.lock");
    let mut attempts = 0u32;
    loop {
        match std::fs::create_dir(&lock) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                attempts += 1;
                if attempts > 20 {
                    anyhow::bail!(
                        "claim lock held after {} retries; remove .stint/claim.lock to reset",
                        attempts
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(e).context("create .stint/claim.lock"),
        }
    }
    let result = f();
    let _ = std::fs::remove_dir(&lock);
    result
}

pub fn print_next(report: &NextReport) {
    let on = color_on();
    if report.ready.is_empty() {
        println!("{} {}", bold("Ready", on), dim("(none)", on));
    } else {
        println!(
            "{} {}",
            bold("Ready", on),
            dim(&format!("({})", report.ready.len()), on)
        );
        for task in &report.ready {
            print_next_task(task, on);
        }
    }
}

/// The reason a task sits in the Blocked section, if any. Dependency blocks take
/// precedence over area contention.
fn blocked_reason(task: &NextTask) -> Option<String> {
    if !task.blockers.is_empty() {
        return Some(format!(
            "blocked by {}",
            format_blockers_inline(&task.blockers)
        ));
    }
    if !task.area_conflicts.is_empty() {
        return Some(format!(
            "area busy: {} in progress",
            task.area_conflicts.join(", ")
        ));
    }
    if !task.selected_conflicts.is_empty() {
        return Some(format!(
            "area taken this run by {}",
            task.selected_conflicts.join(", ")
        ));
    }
    None
}

pub fn print_next_json(repo: &StintRepo, report: &NextReport) {
    use serde_json::{json, Value};

    let tasks_dir = repo.tasks_dir();

    let task_to_json = |t: &NextTask| -> Value {
        let path = tasks_dir
            .join(&t.filename)
            .strip_prefix(repo.stint_dir.parent().unwrap_or(&repo.stint_dir))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| t.filename.clone());
        json!({
            "id": t.id,
            "title": t.title,
            "status": t.status.as_str(),
            "priority": t.priority.as_ref().map(|p| p.as_str()),
            "size": t.size.as_ref().map(|s| s.as_str()),
            "sprint": t.sprint,
            "area": t.area,
            "gh_issue": t.gh_issue,
            "blockers": t.blockers.iter().map(blocker_to_json).collect::<Vec<_>>(),
            "path": path,
        })
    };

    let ready: Vec<Value> = report.ready.iter().map(task_to_json).collect();
    let blocked: Vec<Value> = report.blocked.iter().map(task_to_json).collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "ready": ready, "blocked": blocked })).unwrap()
    );
}

fn blocker_to_json(blocker: &BlockedByRef) -> serde_json::Value {
    use serde_json::json;
    json!({ "ref": blocker.to_string() })
}

fn print_next_task(task: &NextTask, on: bool) {
    let mut meta_label = String::new();
    if let Some(p) = task.priority.as_ref() {
        meta_label.push_str(&format!("[{}]", p));
    }
    if let Some(s) = task.size.as_ref() {
        meta_label.push_str(&format!("[{}]", s));
    }
    if !meta_label.is_empty() {
        meta_label.insert(0, ' ');
    }
    println!(
        "  {}{}  {}",
        paint_id(&task.id, on),
        dim(&meta_label, on),
        task.title
    );
    if !task.area.is_empty() {
        println!("        {}", dim(&task.area.join(" · "), on));
    }
    if let Some(reason) = blocked_reason(task) {
        let mark = if on {
            "✗".yellow().to_string()
        } else {
            "-".to_owned()
        };
        println!("        {} {}", mark, dim(&reason, on));
    }
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
    let id = normalize_sprint_id(id_input);
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
    let sprint = stint::sprint::parse_sprint(&content)
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

/// Append a task to a sprint file. Sprint membership is recorded only in the
/// sprint index — no `sprint` field is written to the task frontmatter.
pub fn cmd_sprint_add(repo: &StintRepo, sprint_id: &str, task_id: &str) -> anyhow::Result<PathBuf> {
    let task_id = resolve_id(task_id);
    let path = repo.resolve_sprint_path(sprint_id)?;
    let content = repo.read_sprint_raw(&path)?;

    // Write the entry as a `../tasks/<filename>` link so editors can `gf` to it.
    // Fall back to the bare ID if the task file does not exist yet.
    let entry = match repo.resolve_task_path(&task_id) {
        Ok(task_path) => task_link(&filename_of(&task_path)),
        Err(_) => task_id.clone(),
    };
    let updated = sprint_add_task(&content, &entry).map_err(|e| anyhow::anyhow!("{}", e))?;
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
    stint::sprint::parse_sprint(&content).with_context(|| {
        format!(
            "sprint file is no longer valid after reorder: {}",
            path.display()
        )
    })?;
    Ok(path)
}

/// Remove a task from a sprint file. Sprint membership is recorded only in the
/// sprint index — no task frontmatter is modified.
pub fn cmd_sprint_remove(
    repo: &StintRepo,
    sprint_id: &str,
    task_id: &str,
) -> anyhow::Result<PathBuf> {
    let task_id = resolve_id(task_id);
    let path = repo.resolve_sprint_path(sprint_id)?;
    let content = repo.read_sprint_raw(&path)?;
    let updated = sprint_remove_task(&content, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
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
    let (tasks, parse_errors) = repo.load_tasks_with_errors()?;
    let sprints = repo.load_sprints()?;
    let direct_resolution = repo.resolve_direct_task_path_blockers(&tasks)?;
    let mut errors: Vec<String> = parse_errors
        .iter()
        .map(|e| format!("{}: {}", e.path.display(), e.error))
        .collect();
    errors.extend(direct_resolution.errors);
    errors.extend(check(&tasks, &sprints).iter().map(|e| e.to_string()));
    Ok(errors)
}

/// Print the `stint status` summary.
pub fn cmd_status(repo: &StintRepo) -> anyhow::Result<()> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let direct_resolution = repo.resolve_direct_task_path_blockers(&tasks)?;
    let report = compute_status_with_resolution(
        &tasks,
        &sprints,
        None,
        &direct_resolution.blocker_resolution,
    );

    let active_count = report.open_count.saturating_sub(report.backlog_count);
    println!("Active:  {} (todo/in-progress)", active_count);
    println!("Backlog: {} (icebox)", report.backlog_count);

    if report.blocked_tasks.is_empty() {
        println!("Blocked:    none");
    } else {
        println!("Blocked ({}):", report.blocked_tasks.len());
        for bt in &report.blocked_tasks {
            println!(
                "  {} {} — {}",
                bt.id,
                bt.title,
                format_blockers_inline(&bt.blocked_by)
            );
        }
    }

    if let Some(p) = &report.sprint_progress {
        println!();
        println!("Sprint {}:", p.sprint_id);
        println!("  Tasks:     {}/{} done", p.done_count, p.task_count);
        println!(
            "  Committed: {}h",
            minutes_to_hours_str(p.committed_minutes)
        );
        println!("  Logged:    {}h", minutes_to_hours_str(p.logged_minutes));
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

/// Build a map from task ID → sprint ID by scanning all sprint index files.
///
/// If a task appears in multiple sprint indexes (which would be a data error),
/// the last sprint listed wins.
fn task_to_sprint_map(sprints: &[Sprint]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for sprint in sprints {
        for entry in &sprint.task_ids {
            let task_id = numeric_prefix(entry).to_owned();
            map.insert(task_id, sprint.header.id.clone());
        }
    }
    map
}

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

/// The bare filename (no directory) of a task path, e.g. `0001-slug.md`.
fn filename_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn format_blockers_inline(blockers: &[BlockedByRef]) -> String {
    blockers
        .iter()
        .map(|blocker| blocker.to_string())
        .collect::<Vec<_>>()
        .join(", ")
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
