/// All command implementations.  Each function takes a `&StintRepo` and
/// whatever arguments the command needs.  No `std::process::exit` here —
/// callers handle exit codes.
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context};
use chrono::{DateTime, SecondsFormat, Utc};
use stint_core::check::check;
use stint_core::duration::Duration;
use stint_core::gate::{
    active_blocker_infos, active_blockers, effective_blocker_infos, gate_applies_to_task,
    BlockerInfo, BlockerSource,
};
use stint_core::mutate::{
    add_actual, new_sprint_content, new_task_content, next_task_id, resolve_id, restart_task,
    set_actual, set_completed_at, set_started_at_if_absent, set_status, title_to_slug,
};
use stint_core::next::{compute_next, NextOptions, NextReport, NextTask};
use stint_core::schema::{BlockedByRef, Task, TaskStatus};
use stint_core::serialize::serialize_task;
use stint_core::sprint::{
    normalize_sprint_id, numeric_prefix, sprint_add_task, sprint_remove_task,
};
use stint_core::status::compute_status;

use crate::repo::StintRepo;

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
        let content = github_issue_task_content(&id, issue);
        repo.write_task(&path, &content)?;
        let task = repo.read_task(&path)?;
        tasks.push(task);
        seen_issues.insert(issue.number.clone());
        imported += 1;
    }

    Ok(GithubImportReport { imported, skipped })
}

fn github_issue_task_content(id: &str, issue: &GithubIssue) -> String {
    let mut task = new_task_content(id, &issue.title);
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

    task
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
    include_all: bool,
    blocked_filter: Option<bool>,
    sprint_filter: Option<&str>,
    area_filter: Option<&str>,
    tag_filter: Option<&str>,
) -> anyhow::Result<Vec<TaskRow>> {
    let tasks = repo.load_tasks()?;
    let gates = repo.load_gates()?;
    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|t| matches!(t.frontmatter.status, TaskStatus::Done))
        .map(|t| t.frontmatter.id.as_str())
        .collect();

    let rows = stint_core::filter::filter_tasks(
        &tasks,
        status_filter,
        sprint_filter,
        area_filter,
        tag_filter,
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
        Some(blocked) => task_is_blocked(t, &gates, &done_ids) == blocked,
        None => true,
    })
    .map(|t| TaskRow {
        id: t.frontmatter.id.clone(),
        title: t.frontmatter.title.clone(),
        status: t.frontmatter.status.as_str().to_owned(),
        blocked: task_is_blocked(t, &gates, &done_ids),
        blockers: active_blocker_infos(t, &gates, &done_ids),
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
    println!(
        "{:<6} {:<11} {:<7} {:<8} {:<6} {}",
        "ID", "STATUS", "BLOCKED", "ESTIMATE", "SPRINT", "TITLE"
    );
    println!("{}", "-".repeat(78));
    for row in rows {
        println!(
            "{:<6} {:<11} {:<7} {:<8} {:<6} {}",
            row.id,
            row.status,
            if row.blocked { "[x]" } else { "[ ]" },
            row.estimate.as_deref().unwrap_or("-"),
            row.sprint.as_deref().unwrap_or("-"),
            truncate(&row.title, 40),
        );
        if row.blocked && !row.blockers.is_empty() {
            println!(
                "       blocked by {}",
                format_blockers_inline(&row.blockers)
            );
        }
    }
}

/// A summarised task row for `stint list` output.
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub blocked: bool,
    pub blockers: Vec<BlockerInfo>,
    pub estimate: Option<String>,
    pub sprint: Option<String>,
}

fn task_is_blocked(
    task: &Task,
    gates: &[stint_core::schema::Gate],
    done_ids: &HashSet<&str>,
) -> bool {
    !active_blockers(task, gates, done_ids).is_empty()
}

/// Print a full task (frontmatter table + body) to stdout.
pub fn cmd_show(repo: &StintRepo, id_input: &str) -> anyhow::Result<()> {
    let path = repo.resolve_task_path(id_input)?;
    let task = repo.read_task(&path)?;
    let tasks = repo.load_tasks()?;
    let gates = repo.load_gates()?;
    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|t| matches!(t.frontmatter.status, TaskStatus::Done))
        .map(|t| t.frontmatter.id.as_str())
        .collect();
    let active_blockers = active_blocker_infos(&task, &gates, &done_ids);
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
    if let Some(started_at) = &fm.started_at {
        println!("Started at:  {}", started_at);
    }
    if let Some(completed_at) = &fm.completed_at {
        println!("Completed at:{}", completed_at);
    }
    if let Some(s) = &fm.sprint {
        println!("Sprint:      {}", s);
    }
    if !active_blockers.is_empty() {
        println!("Blocked by:");
        let direct: Vec<&BlockerInfo> = active_blockers
            .iter()
            .filter(|blocker| matches!(blocker.source, BlockerSource::Direct))
            .collect();
        let inherited: Vec<&BlockerInfo> = active_blockers
            .iter()
            .filter(|blocker| matches!(blocker.source, BlockerSource::Gate(_)))
            .collect();
        if direct.is_empty() {
            println!("  direct:    none");
        } else {
            println!("  direct:    {}", format_blocker_refs(&direct));
        }
        if inherited.is_empty() {
            println!("  inherited: none");
        } else {
            for blocker in inherited {
                println!("  inherited: {}", format_blocker(blocker));
            }
        }
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

pub fn tasks_unblocked_by_done(
    repo: &StintRepo,
    done_id_input: &str,
) -> anyhow::Result<Vec<UnblockedTask>> {
    let done_path = repo.resolve_task_path(done_id_input)?;
    let done_task = repo.read_task(&done_path)?;
    let done_id = done_task.frontmatter.id;
    let tasks = repo.load_tasks()?;
    let gates = repo.load_gates()?;
    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|t| t.frontmatter.status == TaskStatus::Done)
        .map(|t| t.frontmatter.id.as_str())
        .collect();

    let mut unblocked = Vec::new();
    for task in &tasks {
        if !matches!(
            task.frontmatter.status,
            TaskStatus::Backlog | TaskStatus::Todo | TaskStatus::InProgress
        ) {
            continue;
        }

        let waits_on_done_task = effective_blocker_infos(&task, &gates).iter().any(
            |blocker| matches!(&blocker.reference, BlockedByRef::LocalTask(id) if id == &done_id),
        );
        if waits_on_done_task && active_blocker_infos(&task, &gates, &done_ids).is_empty() {
            unblocked.push(UnblockedTask {
                id: task.frontmatter.id.clone(),
                title: task.frontmatter.title.clone(),
            });
        }
    }

    Ok(unblocked)
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

fn warn_if_variance_large(task: &stint_core::schema::Task) {
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

/// Show or claim the next ready task(s).
///
/// When `claim` is true the operation is protected by an advisory lock at
/// `.stint/claim.lock` so that parallel callers each receive a distinct task.
/// Up to `count` tasks are claimed; defaults to 1 when count is None.
pub fn cmd_next(
    repo: &StintRepo,
    sprint: Option<&str>,
    include_area_conflicts: bool,
    claim: bool,
    count: Option<usize>,
) -> anyhow::Result<NextReport> {
    if claim {
        with_claim_lock(repo, || {
            cmd_next_inner(repo, sprint, include_area_conflicts, true, count)
        })
    } else {
        cmd_next_inner(repo, sprint, include_area_conflicts, false, count)
    }
}

fn cmd_next_inner(
    repo: &StintRepo,
    sprint: Option<&str>,
    include_area_conflicts: bool,
    claim: bool,
    count: Option<usize>,
) -> anyhow::Result<NextReport> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let gates = repo.load_gates()?;
    let report = compute_next(
        &tasks,
        &sprints,
        &gates,
        NextOptions {
            sprint,
            include_area_conflicts,
        },
    );

    if claim {
        let n = count.unwrap_or(1);
        for next_task in report.ready.iter().take(n) {
            let path = repo.resolve_task_path(&next_task.id)?;
            let mut task = repo.read_task(&path)?;
            set_status(&mut task, TaskStatus::InProgress);
            set_started_at_if_absent(&mut task, timestamp_or_now(None)?);
            let content = serialize_task(&task);
            repo.write_task(&path, &content)?;
        }
    }

    Ok(report)
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

pub fn print_next(report: &NextReport, claimed: bool, count: Option<usize>) {
    let n = count.unwrap_or(usize::MAX);
    let claimed_ids: Vec<&str> = if claimed {
        report
            .ready
            .iter()
            .take(count.unwrap_or(1))
            .map(|t| t.id.as_str())
            .collect()
    } else {
        vec![]
    };

    if !claimed_ids.is_empty() {
        for id in &claimed_ids {
            if let Some(t) = report.ready.iter().find(|t| t.id.as_str() == *id) {
                println!("claimed: {}", t.id);
                println!("  {}", t.title);
            }
        }
    } else if claimed {
        println!("nothing claimable");
    }

    let visible: Vec<&NextTask> = report.ready.iter().take(n).collect();
    if visible.is_empty() {
        println!("Ready: none");
    } else {
        println!("Ready:");
        for task in &visible {
            print_next_task(task);
        }
    }

    if let Some(bottleneck) = &report.bottleneck {
        println!();
        println!("Bottleneck:");
        println!("  {}", bottleneck.id);
        println!("       {}", bottleneck.title);
        println!("       blocks {} task(s)", bottleneck.blocked_count);
    }
}

pub fn print_next_json(repo: &StintRepo, report: &NextReport, claimed: bool, count: Option<usize>) {
    use serde_json::{json, Value};

    let n = count.unwrap_or(usize::MAX);
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
            "sprint": t.sprint,
            "area": t.area,
            "gh_issue": t.gh_issue,
            "blockers": t.blockers.iter().map(blocker_to_json).collect::<Vec<_>>(),
            "path": path,
        })
    };

    let ready: Vec<Value> = report.ready.iter().take(n).map(task_to_json).collect();
    let blocked: Vec<Value> = report.blocked.iter().map(task_to_json).collect();

    let claimed_ids: Vec<&str> = if claimed {
        report
            .ready
            .iter()
            .take(count.unwrap_or(1))
            .map(|t| t.id.as_str())
            .collect()
    } else {
        vec![]
    };

    let mut obj = json!({ "ready": ready, "blocked": blocked });
    if !claimed_ids.is_empty() {
        obj["claimed"] = json!(claimed_ids);
    } else if claimed {
        obj["claimed"] = json!(Vec::<String>::new());
    }

    println!("{}", serde_json::to_string_pretty(&obj).unwrap());
}

fn blocker_to_json(blocker: &BlockerInfo) -> serde_json::Value {
    use serde_json::json;
    match &blocker.source {
        BlockerSource::Direct => json!({
            "ref": blocker.reference.to_string(),
            "source": "direct",
        }),
        BlockerSource::Gate(gate_id) => json!({
            "ref": blocker.reference.to_string(),
            "source": "gate",
            "gate": gate_id,
        }),
    }
}

fn print_next_task(task: &NextTask) {
    println!("  {}", task.id);
    if !task.area.is_empty() {
        println!("       areas: {}", task.area.join(", "));
    }
    println!("       {}", task.title);
    if !task.blockers.is_empty() {
        println!(
            "       blocked by {}",
            format_blockers_inline(&task.blockers)
        );
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

/// Append a task to a sprint file and update the task's `sprint` frontmatter field.
pub fn cmd_sprint_add(repo: &StintRepo, sprint_id: &str, task_id: &str) -> anyhow::Result<PathBuf> {
    let task_id = resolve_id(task_id);
    let normalized_sprint_id = normalize_sprint_id(sprint_id);
    let path = repo.resolve_sprint_path(sprint_id)?;
    let content = repo.read_sprint_raw(&path)?;
    let updated = sprint_add_task(&content, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    repo.write_sprint(&path, &updated)?;

    // Keep the task's own `sprint` field in sync so filtering/status work.
    if let Ok(task_path) = repo.resolve_task_path(&task_id) {
        let mut task = repo.read_task(&task_path)?;
        task.frontmatter.sprint = Some(normalized_sprint_id);
        let task_content = serialize_task(&task);
        repo.write_task(&task_path, &task_content)?;
    }

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
    stint_core::sprint::parse_sprint(&content).with_context(|| {
        format!(
            "sprint file is no longer valid after reorder: {}",
            path.display()
        )
    })?;
    Ok(path)
}

/// Remove a task from a sprint file and clear the task's `sprint` frontmatter field.
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

    // Clear the task's `sprint` field so filtering/status stay consistent.
    if let Ok(task_path) = repo.resolve_task_path(&task_id) {
        let mut task = repo.read_task(&task_path)?;
        task.frontmatter.sprint = None;
        let task_content = serialize_task(&task);
        repo.write_task(&task_path, &task_content)?;
    }

    Ok(path)
}

// ---------------------------------------------------------------------------
// Gate commands
// ---------------------------------------------------------------------------

pub fn cmd_gates_list(repo: &StintRepo) -> anyhow::Result<()> {
    let gates = repo.load_gates()?;
    if gates.is_empty() {
        println!("No gates.");
        println!("Gates live in .stint/gates/*.md and add inherited blockers to matching tasks.");
        return Ok(());
    }

    let tasks = repo.load_tasks()?;
    println!("Gates:");
    for gate in &gates {
        let matched = tasks
            .iter()
            .filter(|task| gate_applies_to_task(gate, task))
            .count();
        let tags = if gate.applies_to.tags.is_empty() {
            "-".to_owned()
        } else {
            gate.applies_to.tags.join(",")
        };
        let blockers: Vec<String> = gate
            .blocked_by
            .iter()
            .map(|blocker| blocker.to_string())
            .collect();
        println!(
            "  {} tags=[{}] blocked_by=[{}] matches={}",
            gate.id,
            tags,
            blockers.join(","),
            matched
        );
        if let Some(reason) = &gate.reason {
            println!("       {}", reason);
        }
    }
    Ok(())
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
    let (gates, gate_parse_errors) = repo.load_gates_with_errors()?;
    let sprints = repo.load_sprints()?;
    let mut errors: Vec<String> = parse_errors
        .iter()
        .map(|e| format!("{}: {}", e.path.display(), e.error))
        .collect();
    errors.extend(
        gate_parse_errors
            .iter()
            .map(|e| format!("{}: {}", e.path.display(), e.error)),
    );
    errors.extend(
        check(&tasks, &sprints, &gates)
            .iter()
            .map(|e| e.to_string()),
    );
    Ok(errors)
}

/// Print the `stint status` summary.
pub fn cmd_status(repo: &StintRepo) -> anyhow::Result<()> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let gates = repo.load_gates()?;
    let report = compute_status(&tasks, &sprints, &gates, None);

    println!("Open tasks: {}", report.open_count);

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
        return s;
    }
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn format_blockers_inline(blockers: &[BlockerInfo]) -> String {
    blockers
        .iter()
        .map(format_blocker)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_blocker(blocker: &BlockerInfo) -> String {
    match &blocker.source {
        BlockerSource::Direct => blocker.reference.to_string(),
        BlockerSource::Gate(gate_id) => {
            format!("{} via gate {}", blocker.reference, gate_id)
        }
    }
}

fn format_blocker_refs(blockers: &[&BlockerInfo]) -> String {
    blockers
        .iter()
        .map(|blocker| blocker.reference.to_string())
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
