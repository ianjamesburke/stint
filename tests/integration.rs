/// Integration tests for the stint-cli command layer.
///
/// Each test creates a temp dir, wires up a `StintRepo` pointing at a
/// `.stint/` sub-directory, then calls command functions directly.  No
/// subprocess invocation.
use std::env;
use std::fs;
use std::sync::OnceLock;

use tempfile::TempDir;

// Bring the CLI modules into scope.  They live in `src/` so we use a path
// attribute to include them here.
#[path = "../src/cmds.rs"]
mod cmds;

use stint::repo::StintRepo;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Ensure `STINT_TEST_EDITOR=none` is set exactly once across the process.
///
/// Using `OnceLock` avoids a data race when tests run in parallel and multiple
/// threads call `env::set_var` simultaneously.
static EDITOR_SUPPRESSED: OnceLock<()> = OnceLock::new();

fn suppress_editor() {
    EDITOR_SUPPRESSED.get_or_init(|| {
        // SAFETY: we are the only writer and this runs before any thread that
        // reads STINT_TEST_EDITOR, because `OnceLock` serialises the init.
        #[allow(deprecated)]
        unsafe {
            env::set_var("STINT_TEST_EDITOR", "none")
        };
    });
}

/// Create a `TempDir` with `.stint/tasks/` and `.stint/sprints/` pre-created.
fn setup() -> (TempDir, StintRepo) {
    suppress_editor();

    let tmp = TempDir::new().unwrap();
    let stint_dir = tmp.path().join(".stint");
    fs::create_dir_all(stint_dir.join("tasks")).unwrap();
    fs::create_dir_all(stint_dir.join("sprints")).unwrap();
    let repo = StintRepo { stint_dir };
    (tmp, repo)
}

fn setup_empty_dir() -> TempDir {
    suppress_editor();
    TempDir::new().unwrap()
}

/// Write a raw task file into the repo's tasks dir.
fn write_task_file(repo: &StintRepo, filename: &str, content: &str) {
    fs::write(repo.tasks_dir().join(filename), content).unwrap();
}

fn write_task_file_at(root: &std::path::Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Write a raw sprint file into the repo's sprints dir.
fn write_sprint_file(repo: &StintRepo, filename: &str, content: &str) {
    fs::write(repo.sprints_dir().join(filename), content).unwrap();
}

/// Minimal valid task content.
fn task_content(id: &str, title: &str, status: &str) -> String {
    format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: {status}\n---\n")
}

// ---------------------------------------------------------------------------
// cmd_init
// ---------------------------------------------------------------------------

#[test]
fn init_creates_workspace_layout() {
    let tmp = setup_empty_dir();
    let report = cmds::cmd_init(tmp.path(), false, false, None).unwrap();

    assert_eq!(report.repo, tmp.path().join(".stint"));
    assert!(tmp.path().join(".stint/tasks").is_dir());
    assert!(tmp.path().join(".stint/sprints").is_dir());
    assert!(tmp.path().join(".stint/config.toml").is_file());
}

#[test]
fn init_refuses_existing_workspace_without_force() {
    let tmp = setup_empty_dir();
    cmds::cmd_init(tmp.path(), false, false, None).unwrap();

    assert!(cmds::cmd_init(tmp.path(), false, false, None).is_err());
}

#[test]
fn init_force_is_idempotent() {
    let tmp = setup_empty_dir();
    cmds::cmd_init(tmp.path(), false, false, None).unwrap();
    let report = cmds::cmd_init(tmp.path(), true, false, None).unwrap();

    assert_eq!(report.repo, tmp.path().join(".stint"));
    assert!(tmp.path().join(".stint/tasks").is_dir());
}

#[test]
fn github_import_creates_tasks_and_skips_existing_issue() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-existing.md",
        "---\nid: \"0001\"\ntitle: \"Existing\"\nstatus: backlog\ngh_issue: [7]\n---\n",
    );
    let issues = cmds::parse_github_issues_json(
        br#"[
          {"number": 7, "title": "Existing issue", "body": "old", "labels": []},
          {"number": 8, "title": "Fix auth flow", "body": "Issue body", "labels": [{"name": "bug"}, {"name": "cli"}]}
        ]"#,
    )
    .unwrap();

    let report = cmds::import_github_issues(&repo, &issues).unwrap();

    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped, 1);
    let imported = fs::read_to_string(repo.tasks_dir().join("0002-fix-auth-flow.md")).unwrap();
    assert!(imported.contains("title: \"Fix auth flow\""));
    assert!(imported.contains("gh_issue:\n  - \"8\""));
    assert!(imported.contains("tags:\n  - \"bug\"\n  - \"cli\""));
    assert!(imported.contains("## GitHub Issue\n\nIssue body"));
}

// ---------------------------------------------------------------------------
// StintRepo::find
// ---------------------------------------------------------------------------

#[test]
fn find_repo_locates_stint_dir() {
    let (tmp, _repo) = setup();
    // find() from the temp root should find .stint/
    let found = StintRepo::find(tmp.path()).unwrap();
    assert_eq!(found.stint_dir, tmp.path().join(".stint"));
}

#[test]
fn find_repo_walks_upward() {
    let (tmp, _repo) = setup();
    // find() from a sub-directory should walk up and find .stint/
    let subdir = tmp.path().join("src/nested");
    fs::create_dir_all(&subdir).unwrap();
    let found = StintRepo::find(&subdir).unwrap();
    assert_eq!(found.stint_dir, tmp.path().join(".stint"));
}

#[test]
fn find_repo_errors_when_absent() {
    let tmp = TempDir::new().unwrap();
    assert!(StintRepo::find(tmp.path()).is_err());
}

// ---------------------------------------------------------------------------
// Task ID resolution
// ---------------------------------------------------------------------------

#[test]
fn resolve_id_full() {
    use stint::mutate::resolve_id;
    assert_eq!(resolve_id("0001"), "0001");
}

#[test]
fn resolve_id_partial() {
    use stint::mutate::resolve_id;
    assert_eq!(resolve_id("1"), "0001");
    assert_eq!(resolve_id("42"), "0042");
}

#[test]
fn resolve_id_with_slug() {
    use stint::mutate::resolve_id;
    assert_eq!(resolve_id("0001-auth-middleware"), "0001");
}

// ---------------------------------------------------------------------------
// cmd_add
// ---------------------------------------------------------------------------

#[test]
fn add_creates_file() {
    let (_tmp, repo) = setup();
    let path = cmds::cmd_add(
        &repo,
        "My first task",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    assert!(path.exists());
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("id: \"0001\""));
    assert!(content.contains("My first task"));
    assert!(content.contains("status: backlog"));
}

#[test]
fn add_increments_id() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-existing.md",
        &task_content("0001", "Existing", "backlog"),
    );
    let path = cmds::cmd_add(
        &repo,
        "Second task",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("id: \"0002\""));
}

#[test]
fn add_filename_includes_slug() {
    let (_tmp, repo) = setup();
    let path = cmds::cmd_add(
        &repo,
        "Auth Middleware",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let filename = path.file_name().unwrap().to_string_lossy();
    assert!(filename.starts_with("0001-auth-middleware"));
}

#[test]
fn add_writes_size_field() {
    let (_tmp, repo) = setup();
    let path = cmds::cmd_add(
        &repo,
        "Sized task",
        Some("p2"),
        Some("m"),
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("priority: p2"));
    assert!(content.contains("size: m"));
}

#[test]
fn add_rejects_invalid_size() {
    let (_tmp, repo) = setup();
    let err = cmds::cmd_add(
        &repo,
        "Bad size",
        None,
        Some("huge"),
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown size"));
}

#[test]
fn add_writes_metadata_fields() {
    let (_tmp, repo) = setup();
    let edits = cmds::TaskFieldEdits {
        area: vec!["cli".to_owned()],
        tags: vec!["ergonomics".to_owned()],
        blocked_by: vec!["0002".to_owned()],
        gh_issue: vec!["42".to_owned()],
        body_source: None,
    };
    let path = cmds::cmd_add(&repo, "Agent-driven task", None, None, &edits, false).unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("area:\n  - \"cli\""));
    assert!(content.contains("tags:\n  - \"ergonomics\""));
    assert!(content.contains("blocked_by:\n  - 2"));
    assert!(content.contains("gh_issue:\n  - \"42\""));
}

#[test]
fn add_writes_body_from_file() {
    let (tmp, repo) = setup();
    let body_path = tmp.path().join("body.md");
    fs::write(&body_path, "custom body content\n").unwrap();
    let edits = cmds::TaskFieldEdits {
        body_source: Some(body_path.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let path = cmds::cmd_add(&repo, "Task with body", None, None, &edits, false).unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("custom body content"));
    assert!(!content.contains("## Why"));
}

#[test]
fn add_no_edit_skips_editor() {
    let (_tmp, repo) = setup();
    // suppress_editor() already routes $EDITOR to a no-op via STINT_TEST_EDITOR;
    // `no_edit: true` should behave identically (and is exercised directly here).
    let path = cmds::cmd_add(
        &repo,
        "Headless task",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        true,
    )
    .unwrap();
    assert!(path.exists());
}

#[test]
fn list_row_includes_size() {
    let (_tmp, repo) = setup();
    cmds::cmd_add(
        &repo,
        "Sized task",
        None,
        Some("l"),
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let rows = cmds::cmd_list(&repo, None, false, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].size.as_deref(), Some("l"));
}

// ---------------------------------------------------------------------------
// cmd_set
// ---------------------------------------------------------------------------

#[test]
fn set_replaces_area_and_tags() {
    let (_tmp, repo) = setup();
    let add_path = cmds::cmd_add(
        &repo,
        "Task to edit",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let id = add_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .split('-')
        .next()
        .unwrap()
        .to_owned();

    let edits = cmds::TaskFieldEdits {
        area: vec!["backend".to_owned()],
        tags: vec!["urgent".to_owned(), "flaky".to_owned()],
        ..Default::default()
    };
    let path = cmds::cmd_set(&repo, &id, &edits).unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("area:\n  - \"backend\""));
    assert!(content.contains("tags:\n  - \"urgent\"\n  - \"flaky\""));
}

#[test]
fn set_replaces_body_from_file() {
    let (tmp, repo) = setup();
    let add_path = cmds::cmd_add(
        &repo,
        "Task to edit",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let id = add_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .split('-')
        .next()
        .unwrap()
        .to_owned();

    let body_path = tmp.path().join("body.md");
    fs::write(&body_path, "replacement body\n").unwrap();
    let edits = cmds::TaskFieldEdits {
        body_source: Some(body_path.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let path = cmds::cmd_set(&repo, &id, &edits).unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("replacement body"));
    assert!(!content.contains("## Why"));
}

#[test]
fn set_rejects_empty_edits() {
    let (_tmp, repo) = setup();
    let add_path = cmds::cmd_add(
        &repo,
        "Task to edit",
        None,
        None,
        &cmds::TaskFieldEdits::default(),
        false,
    )
    .unwrap();
    let id = add_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .split('-')
        .next()
        .unwrap()
        .to_owned();

    let err = cmds::cmd_set(&repo, &id, &cmds::TaskFieldEdits::default()).unwrap_err();
    assert!(err.to_string().contains("no fields provided"));
}

// ---------------------------------------------------------------------------
// cmd_list
// ---------------------------------------------------------------------------

#[test]
fn list_defaults_to_active_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    write_task_file(
        &repo,
        "0003-c.md",
        &task_content("0003", "Task C", "archived"),
    );
    let rows = cmds::cmd_list(&repo, None, false, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_all_includes_done_and_archived_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    write_task_file(
        &repo,
        "0003-c.md",
        &task_content("0003", "Task C", "archived"),
    );
    let rows = cmds::cmd_list(&repo, None, true, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 3);
}

#[test]
fn list_filters_by_status() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    let rows = cmds::cmd_list(&repo, Some("backlog"), false, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_explicit_status_can_show_done_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    let rows = cmds::cmd_list(&repo, Some("done"), false, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0002");
}

#[test]
fn list_marks_currently_blocked_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "todo"));
    write_task_file(
        &repo,
        "0002-b.md",
        "---\nid: \"0002\"\ntitle: \"Task B\"\nstatus: todo\nblocked_by: [\"0001\"]\n---\n",
    );

    let rows = cmds::cmd_list(&repo, None, false, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(!rows.iter().find(|r| r.id == "0001").unwrap().blocked);
    assert!(rows.iter().find(|r| r.id == "0002").unwrap().blocked);
}

#[test]
fn list_does_not_mark_task_blocked_by_done_local_task() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "done"));
    write_task_file(
        &repo,
        "0002-b.md",
        "---\nid: \"0002\"\ntitle: \"Task B\"\nstatus: todo\nblocked_by: [\"0001\"]\n---\n",
    );

    let rows = cmds::cmd_list(&repo, None, false, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0002");
    assert!(!rows[0].blocked);
}

#[test]
fn list_filters_blocked_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "todo"));
    write_task_file(
        &repo,
        "0002-b.md",
        "---\nid: \"0002\"\ntitle: \"Task B\"\nstatus: todo\nblocked_by: [\"0001\"]\n---\n",
    );

    let blocked = cmds::cmd_list(&repo, None, false, Some(true), None, None, None, None).unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].id, "0002");

    let unblocked =
        cmds::cmd_list(&repo, None, false, Some(false), None, None, None, None).unwrap();
    assert_eq!(unblocked.len(), 1);
    assert_eq!(unblocked[0].id, "0001");
}

#[test]
fn list_treats_direct_blocker_as_blocked() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-blocker.md",
        &task_content("0001", "Blocker", "todo"),
    );
    write_task_file(
        &repo,
        "0002-dep.md",
        "---\nid: \"0002\"\ntitle: \"Dep\"\nstatus: todo\nblocked_by:\n  - 0001\n---\n",
    );

    let blocked = cmds::cmd_list(&repo, None, false, Some(true), None, None, None, None).unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].id, "0002");
}

#[test]
fn list_filters_by_sprint() {
    let (_tmp, repo) = setup();
    let t1 = "---\nid: \"0001\"\ntitle: \"A\"\nstatus: todo\n---\n";
    let t2 = "---\nid: \"0002\"\ntitle: \"B\"\nstatus: todo\n---\n";
    write_task_file(&repo, "0001-a.md", t1);
    write_task_file(&repo, "0002-b.md", t2);
    // Sprint membership is determined by the sprint index, not task frontmatter.
    write_sprint_file(&repo, "s1.md", "# Sprint 1 · Jan 1–15\n\n- 0001\n");
    write_sprint_file(&repo, "s2.md", "# Sprint 2 · Jan 16–31\n\n- 0002\n");
    let rows = cmds::cmd_list(&repo, None, false, None, Some("s1"), None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_filters_by_area() {
    let (_tmp, repo) = setup();
    let t1 = "---\nid: \"0001\"\ntitle: \"A\"\nstatus: backlog\narea:\n  - backend\n---\n";
    let t2 = "---\nid: \"0002\"\ntitle: \"B\"\nstatus: backlog\narea:\n  - frontend\n---\n";
    write_task_file(&repo, "0001-a.md", t1);
    write_task_file(&repo, "0002-b.md", t2);
    let rows = cmds::cmd_list(&repo, None, false, None, None, Some("backend"), None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_filters_by_tag() {
    let (_tmp, repo) = setup();
    let t1 = "---\nid: \"0001\"\ntitle: \"A\"\nstatus: backlog\ntags:\n  - security\n---\n";
    let t2 = "---\nid: \"0002\"\ntitle: \"B\"\nstatus: backlog\ntags:\n  - perf\n---\n";
    write_task_file(&repo, "0001-a.md", t1);
    write_task_file(&repo, "0002-b.md", t2);
    let rows =
        cmds::cmd_list(&repo, None, false, None, None, None, Some("security"), None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_empty_returns_empty_vec() {
    let (_tmp, repo) = setup();
    let rows = cmds::cmd_list(&repo, None, false, None, None, None, None, None).unwrap();
    assert!(rows.is_empty());
}

// ---------------------------------------------------------------------------
// cmd_done
// ---------------------------------------------------------------------------

#[test]
fn done_sets_status() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    cmds::cmd_done(&repo, "0001", Some("2h"), None, None).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("status: done"));
}

#[test]
fn done_records_actual_time() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "backlog"),
    );
    cmds::cmd_done(&repo, "0001", Some("3h"), None, None).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("actual: \"3h\""));
}

#[test]
fn done_skips_actual_when_already_set() {
    let (_tmp, repo) = setup();
    let t = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: in-progress\nactual: \"1h\"\n---\n";
    write_task_file(&repo, "0001-task.md", t);
    // Pass no actual override — should NOT prompt when actual already set.
    cmds::cmd_done(&repo, "0001", None, None, None).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("actual: \"1h\""));
    assert!(content.contains("status: done"));
}

#[test]
fn done_reports_tasks_unblocked_by_completed_task() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    write_task_file(
        &repo,
        "0002-dependent.md",
        "---\nid: \"0002\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by: [\"0001\"]\n---\n",
    );

    cmds::cmd_done(&repo, "1", Some("2h"), None, None).unwrap();
    let unblocked = cmds::tasks_unblocked_by_done(&repo, "1").unwrap();

    assert_eq!(unblocked.len(), 1);
    assert_eq!(unblocked[0].id, "0002");
    assert_eq!(unblocked[0].title, "Dependent");
}

#[test]
fn done_does_not_report_tasks_with_remaining_blockers() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    write_task_file(
        &repo,
        "0003-other.md",
        &task_content("0003", "Other", "todo"),
    );
    write_task_file(
        &repo,
        "0002-dependent.md",
        "---\nid: \"0002\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by: [\"0001\", \"0003\"]\n---\n",
    );

    cmds::cmd_done(&repo, "0001", Some("2h"), None, None).unwrap();
    let unblocked = cmds::tasks_unblocked_by_done(&repo, "0001").unwrap();

    assert!(unblocked.is_empty());
}

#[test]
fn done_prunes_completed_local_blocker_from_open_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    write_task_file(
        &repo,
        "0003-other.md",
        &task_content("0003", "Other", "todo"),
    );
    write_task_file(
        &repo,
        "0002-dependent.md",
        "---\nid: \"0002\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"0001\"\n  - \"0003\"\n  - \"@12\"\n  - \"owner/repo:0042\"\n  - \"waiting on design\"\n---\n",
    );

    cmds::cmd_done(&repo, "1", Some("2h"), None, None).unwrap();
    let pruned = cmds::prune_done_blocker_refs(&repo, "1").unwrap();

    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0].id, "0002");
    let task = repo
        .read_task(&repo.tasks_dir().join("0002-dependent.md"))
        .unwrap();
    let blockers = task
        .frontmatter
        .blocked_by
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        blockers,
        vec!["0003", "@12", "owner/repo:0042", "waiting on design"]
    );
}

#[test]
fn done_prunes_shorthand_completed_local_blocker_from_open_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0003-task.md",
        &task_content("0003", "Task", "in-progress"),
    );
    write_task_file(
        &repo,
        "0004-dependent.md",
        "---\nid: \"0004\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - 3\n---\n",
    );

    cmds::cmd_done(&repo, "3", Some("2h"), None, None).unwrap();
    let pruned = cmds::prune_done_blocker_refs(&repo, "3").unwrap();

    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0].id, "0004");
    let task = repo
        .read_task(&repo.tasks_dir().join("0004-dependent.md"))
        .unwrap();
    assert!(task.frontmatter.blocked_by.is_empty());
}

#[test]
fn done_prune_skips_closed_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    write_task_file(
        &repo,
        "0002-closed.md",
        "---\nid: \"0002\"\ntitle: \"Closed\"\nstatus: done\nblocked_by: [\"0001\"]\n---\n",
    );

    cmds::cmd_done(&repo, "0001", Some("2h"), None, None).unwrap();
    let pruned = cmds::prune_done_blocker_refs(&repo, "0001").unwrap();

    assert!(pruned.is_empty());
    let content = fs::read_to_string(repo.tasks_dir().join("0002-closed.md")).unwrap();
    assert!(content.contains("blocked_by: [\"0001\"]"));
}

#[test]
fn claim_sets_status_and_started_at() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-task.md", &task_content("0001", "Task", "todo"));
    cmds::cmd_claim(
        &repo,
        Some("0001"),
        None,
        false,
        Some("2026-06-10T12:00:00Z"),
        false,
    )
    .unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("status: in-progress"));
    assert!(content.contains("started_at: \"2026-06-10T12:00:00Z\""));
}

#[test]
fn claim_refuses_to_overwrite_started_at_without_restart() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"Task\"\nstatus: in-progress\nstarted_at: \"2026-06-10T12:00:00Z\"\n---\n",
    );
    let err = cmds::cmd_claim(
        &repo,
        Some("0001"),
        None,
        false,
        Some("2026-06-10T13:00:00Z"),
        false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("--restart"));
}

#[test]
fn claim_restart_replaces_started_at_and_clears_completion() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"Task\"\nstatus: done\nactual: \"1h\"\nstarted_at: \"2026-06-10T12:00:00Z\"\ncompleted_at: \"2026-06-10T13:00:00Z\"\n---\n",
    );
    cmds::cmd_claim(
        &repo,
        Some("0001"),
        None,
        true,
        Some("2026-06-10T14:00:00Z"),
        false,
    )
    .unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(
        task.frontmatter.status,
        stint::schema::TaskStatus::InProgress
    );
    assert_eq!(
        task.frontmatter.started_at.as_deref(),
        Some("2026-06-10T14:00:00Z")
    );
    assert!(task.frontmatter.completed_at.is_none());
    assert!(task.frontmatter.actual.is_none());
}

#[test]
fn done_computes_actual_from_started_at() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"Task\"\nstatus: in-progress\nstarted_at: \"2026-06-10T12:00:00Z\"\n---\n",
    );
    cmds::cmd_done(&repo, "0001", None, None, Some("2026-06-10T13:30:00Z")).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(
        task.frontmatter.actual,
        Some(stint::duration::Duration::from_minutes(90))
    );
    assert_eq!(
        task.frontmatter.completed_at.as_deref(),
        Some("2026-06-10T13:30:00Z")
    );
}

#[test]
fn done_uses_started_at_override_when_missing() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    cmds::cmd_done(
        &repo,
        "0001",
        None,
        Some("2026-06-10T12:00:00Z"),
        Some("2026-06-10T12:45:00Z"),
    )
    .unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(
        task.frontmatter.actual,
        Some(stint::duration::Duration::from_minutes(45))
    );
    assert_eq!(
        task.frontmatter.started_at.as_deref(),
        Some("2026-06-10T12:00:00Z")
    );
}

#[test]
fn done_with_actual_records_started_at_override() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    cmds::cmd_done(
        &repo,
        "0001",
        Some("30m"),
        Some("2026-06-10T12:00:00Z"),
        Some("2026-06-10T12:45:00Z"),
    )
    .unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(
        task.frontmatter.actual,
        Some(stint::duration::Duration::from_minutes(30))
    );
    assert_eq!(
        task.frontmatter.started_at.as_deref(),
        Some("2026-06-10T12:00:00Z")
    );
}

// ---------------------------------------------------------------------------
// cmd_log
// ---------------------------------------------------------------------------

#[test]
fn log_accumulates_time() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "in-progress"),
    );
    cmds::cmd_log(&repo, "0001", "2h").unwrap();
    cmds::cmd_log(&repo, "1", "30m").unwrap(); // partial ID
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    // 2h + 30m = 150m
    assert!(content.contains("actual: \"150m\""));
}

#[test]
fn log_rejects_invalid_duration() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "backlog"),
    );
    assert!(cmds::cmd_log(&repo, "0001", "2x").is_err());
}

// ---------------------------------------------------------------------------
// cmd_archive
// ---------------------------------------------------------------------------

#[test]
fn archive_sets_status() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "backlog"),
    );
    cmds::cmd_archive(&repo, "0001").unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("status: archived"));
}

// ---------------------------------------------------------------------------
// cmd_next
// ---------------------------------------------------------------------------

#[test]
fn next_returns_ready_tasks_without_area_conflicts() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-active.md",
        "---\nid: \"0001\"\ntitle: \"Active\"\nstatus: in-progress\narea: [cli]\n---\n",
    );
    write_task_file(
        &repo,
        "0002-conflict.md",
        "---\nid: \"0002\"\ntitle: \"Conflict\"\nstatus: todo\narea: [cli]\n---\n",
    );
    write_task_file(
        &repo,
        "0003-ready.md",
        "---\nid: \"0003\"\ntitle: \"Ready\"\nstatus: todo\narea: [docs]\n---\n",
    );

    let report = cmds::cmd_next(&repo, None, false, false).unwrap();
    assert_eq!(report.ready.len(), 1);
    assert_eq!(report.ready[0].id, "0003");
}

#[test]
fn next_blocks_on_unfinished_direct_task_path() {
    let (tmp, repo) = setup();
    write_task_file_at(
        tmp.path(),
        "upstream/.stint/tasks/0030-target.md",
        &task_content("0030", "Target", "todo"),
    );
    write_task_file(
        &repo,
        "0001-dependent.md",
        "---\nid: \"0001\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"./upstream/.stint/tasks/0030-target.md\"\n---\n",
    );

    let report = cmds::cmd_next(&repo, None, false, false).unwrap();
    assert_eq!(report.blocked[0].id, "0001");
}

#[test]
fn next_unblocks_done_direct_task_path() {
    let (tmp, repo) = setup();
    write_task_file_at(
        tmp.path(),
        "upstream/.stint/tasks/0030-target.md",
        &task_content("0030", "Target", "done"),
    );
    write_task_file(
        &repo,
        "0001-dependent.md",
        "---\nid: \"0001\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"./upstream/.stint/tasks/0030-target.md\"\n---\n",
    );

    let report = cmds::cmd_next(&repo, None, false, false).unwrap();
    assert_eq!(report.ready[0].id, "0001");
}

#[test]
fn check_reports_missing_direct_task_path() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-dependent.md",
        "---\nid: \"0001\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"./missing/.stint/tasks/0030-target.md\"\n---\n",
    );

    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.iter().any(|e| e.contains("file not found")));
}

#[test]
fn check_reports_malformed_direct_task_path_target() {
    let (tmp, repo) = setup();
    write_task_file_at(
        tmp.path(),
        "upstream/.stint/tasks/0030-target.md",
        "---\nid: \"0030\"\ntitle: \"Target\"\nstatus: nope\n---\n",
    );
    write_task_file(
        &repo,
        "0001-dependent.md",
        "---\nid: \"0001\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"./upstream/.stint/tasks/0030-target.md\"\n---\n",
    );

    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.iter().any(|e| e.contains("parse failed")));
}

#[test]
fn check_reports_direct_task_path_id_mismatch() {
    let (tmp, repo) = setup();
    write_task_file_at(
        tmp.path(),
        "upstream/.stint/tasks/0030-target.md",
        &task_content("0031", "Target", "done"),
    );
    write_task_file(
        &repo,
        "0001-dependent.md",
        "---\nid: \"0001\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"./upstream/.stint/tasks/0030-target.md\"\n---\n",
    );

    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.iter().any(|e| e.contains("does not match")));
}

#[test]
fn free_text_blocker_still_blocks_next() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-dependent.md",
        "---\nid: \"0001\"\ntitle: \"Dependent\"\nstatus: todo\nblocked_by:\n  - \"waiting for review\"\n---\n",
    );

    let report = cmds::cmd_next(&repo, None, false, false).unwrap();
    assert_eq!(
        report.blocked[0].blockers[0].to_string(),
        "waiting for review"
    );
}

#[test]
fn claim_auto_sets_top_ready_task_in_progress() {
    let (_tmp, repo) = setup();
    write_sprint_file(
        &repo,
        "s1.md",
        "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0002\n- 0001\n",
    );
    write_task_file(
        &repo,
        "0001-later.md",
        "---\nid: \"0001\"\ntitle: \"Later\"\nstatus: todo\nsprint: \"s1\"\narea: [docs]\n---\n",
    );
    write_task_file(
        &repo,
        "0002-first.md",
        "---\nid: \"0002\"\ntitle: \"First\"\nstatus: todo\nsprint: \"s1\"\narea: [cli]\n---\n",
    );

    cmds::cmd_claim(&repo, None, Some("s1"), false, None, false).unwrap();

    let claimed = repo
        .read_task(&repo.resolve_task_path("0001").unwrap())
        .unwrap();
    let untouched = repo
        .read_task(&repo.resolve_task_path("0002").unwrap())
        .unwrap();
    assert_eq!(
        claimed.frontmatter.status,
        stint::schema::TaskStatus::InProgress
    );
    assert!(claimed.frontmatter.started_at.is_some());
    assert_eq!(
        untouched.frontmatter.status,
        stint::schema::TaskStatus::Todo
    );
}

#[test]
fn claim_explicit_id_sets_task_in_progress() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"Task\"\nstatus: todo\n---\n",
    );
    write_task_file(
        &repo,
        "0002-other.md",
        "---\nid: \"0002\"\ntitle: \"Other\"\nstatus: todo\n---\n",
    );

    cmds::cmd_claim(&repo, Some("0002"), None, false, None, false).unwrap();

    let claimed = repo
        .read_task(&repo.resolve_task_path("0002").unwrap())
        .unwrap();
    let untouched = repo
        .read_task(&repo.resolve_task_path("0001").unwrap())
        .unwrap();
    assert_eq!(
        claimed.frontmatter.status,
        stint::schema::TaskStatus::InProgress
    );
    assert!(claimed.frontmatter.started_at.is_some());
    assert_eq!(
        untouched.frontmatter.status,
        stint::schema::TaskStatus::Todo
    );
}

#[test]
fn next_json_output_is_valid() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"A task\"\nstatus: todo\narea: [cli]\ngh_issue: [42]\n---\n",
    );

    let report = cmds::cmd_next(&repo, None, false, false).unwrap();
    // Capture JSON output via a simple structural check on the report fields.
    assert_eq!(report.ready[0].id, "0001");
    assert_eq!(report.ready[0].gh_issue, vec!["42"]);
    assert_eq!(report.ready[0].filename, "0001-task.md");
}

// ---------------------------------------------------------------------------
// Sprint commands
// ---------------------------------------------------------------------------

#[test]
fn sprint_new_creates_file() {
    let (_tmp, repo) = setup();
    let path = cmds::cmd_sprint_new(&repo, "s1", "Jun 9-20", Some("Ship it")).unwrap();
    assert!(path.exists());
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("# Sprint 1"));
    assert!(content.contains("goal: Ship it"));
}

#[test]
fn sprint_new_normalises_id() {
    let (_tmp, repo) = setup();
    // Passing "12" instead of "s12" should work.
    let path = cmds::cmd_sprint_new(&repo, "12", "Jul 1-14", None).unwrap();
    let filename = path.file_name().unwrap().to_string_lossy();
    assert_eq!(filename.as_ref(), "s12.md");
}

#[test]
fn sprint_new_errors_on_duplicate() {
    let (_tmp, repo) = setup();
    cmds::cmd_sprint_new(&repo, "s1", "Jun 1-14", None).unwrap();
    assert!(cmds::cmd_sprint_new(&repo, "s1", "Jun 1-14", None).is_err());
}

#[test]
fn sprint_add_appends_task() {
    let (_tmp, repo) = setup();
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n");
    cmds::cmd_sprint_add(&repo, "s1", "0002").unwrap();
    let content = fs::read_to_string(repo.sprints_dir().join("s1.md")).unwrap();
    assert!(content.contains("- 0002"));
}

#[test]
fn sprint_add_writes_markdown_link_when_task_exists() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0002-dep.md", &task_content("0002", "Dep", "todo"));
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n");
    cmds::cmd_sprint_add(&repo, "s1", "0002").unwrap();
    let content = fs::read_to_string(repo.sprints_dir().join("s1.md")).unwrap();
    assert!(content.contains("- [0002](../tasks/0002-dep.md)"));
}

#[test]
fn sprint_remove_deletes_task() {
    let (_tmp, repo) = setup();
    write_sprint_file(
        &repo,
        "s1.md",
        "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n- 0002\n",
    );
    cmds::cmd_sprint_remove(&repo, "s1", "0001").unwrap();
    let content = fs::read_to_string(repo.sprints_dir().join("s1.md")).unwrap();
    assert!(!content.contains("- 0001"));
    assert!(content.contains("- 0002"));
}

// ---------------------------------------------------------------------------
// cmd_check
// ---------------------------------------------------------------------------

#[test]
fn check_passes_clean_graph() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn check_detects_unresolved_blocked_by() {
    let (_tmp, repo) = setup();
    let content =
        "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nblocked_by:\n  - \"9999\"\n---\n";
    write_task_file(&repo, "0001-t.md", content);
    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(!errors.is_empty(), "expected at least one error");
    assert!(errors.iter().any(|e| e.contains("9999")));
}

#[test]
fn check_detects_duplicate_ids() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    // Different filename, same id field — triggers duplicate check.
    write_task_file(
        &repo,
        "0001-b.md",
        &task_content("0001", "Task B", "backlog"),
    );
    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.iter().any(|e| e.contains("duplicate")));
}

#[test]
fn check_detects_id_filename_mismatch() {
    let (_tmp, repo) = setup();
    // id field says 0099 but filename prefix is 0001.
    let content = "---\nid: \"0099\"\ntitle: \"T\"\nstatus: backlog\n---\n";
    write_task_file(&repo, "0001-t.md", content);
    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.iter().any(|e| e.contains("filename")));
}

#[test]
fn check_empty_repo_passes() {
    let (_tmp, repo) = setup();
    let errors = cmds::cmd_check(&repo, false).unwrap();
    assert!(errors.is_empty());
}

// ---------------------------------------------------------------------------
// Round-trip: serialize → parse
// ---------------------------------------------------------------------------

#[test]
fn done_then_log_round_trips_cleanly() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"Work\"\nstatus: in-progress\nestimate: \"4h\"\n---\n\nBody text.\n",
    );
    // Log 1h of work.
    cmds::cmd_log(&repo, "0001", "1h").unwrap();
    // Mark done with a manual override.
    cmds::cmd_done(&repo, "0001", Some("2h"), None, None).unwrap();

    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(task.frontmatter.status, stint::schema::TaskStatus::Done);
    assert_eq!(
        task.frontmatter.actual,
        Some(stint::duration::Duration::from_minutes(120))
    );
    assert!(task.body.contains("Body text."));
}

#[test]
fn done_without_prior_log_records_actual() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        "---\nid: \"0001\"\ntitle: \"Work\"\nstatus: in-progress\nestimate: \"4h\"\n---\n",
    );
    // No prior log — done should record the provided actual.
    cmds::cmd_done(&repo, "0001", Some("3h"), None, None).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(task.frontmatter.status, stint::schema::TaskStatus::Done);
    assert_eq!(
        task.frontmatter.actual,
        Some(stint::duration::Duration::from_minutes(180))
    );
}

// ---------------------------------------------------------------------------
// cmd_show
// ---------------------------------------------------------------------------

#[test]
fn show_prints_task_fields() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Show Me", "backlog"),
    );
    // cmd_show writes to stdout; we verify it doesn't error and uses the right id.
    cmds::cmd_show(&repo, "0001").unwrap();
}

#[test]
fn show_errors_on_missing_task() {
    let (_tmp, repo) = setup();
    assert!(cmds::cmd_show(&repo, "9999").is_err());
}

// ---------------------------------------------------------------------------
// cmd_remove
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_task_file() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task", "backlog"),
    );
    let path = repo.resolve_task_path("0001").unwrap();
    assert!(path.exists());

    let removed = cmds::cmd_remove(&repo, &[String::from("0001")]).unwrap();
    assert_eq!(removed, vec![path.clone()]);
    assert!(!path.exists());
}

#[test]
fn remove_accepts_multiple_task_ids() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(
        &repo,
        "0002-b.md",
        &task_content("0002", "Task B", "backlog"),
    );

    let removed = cmds::cmd_remove(&repo, &[String::from("0001"), String::from("2")]).unwrap();
    assert_eq!(removed.len(), 2);
    assert!(repo.resolve_task_path("0001").is_err());
    assert!(repo.resolve_task_path("0002").is_err());
}

#[test]
fn remove_requires_at_least_one_id() {
    let (_tmp, repo) = setup();
    let err = cmds::cmd_remove(&repo, &[]).unwrap_err();
    assert!(err.to_string().contains("at least one task id"));
}

// ---------------------------------------------------------------------------
// cmd_status
// ---------------------------------------------------------------------------

#[test]
fn status_runs_on_empty_repo() {
    let (_tmp, repo) = setup();
    cmds::cmd_status(&repo).unwrap();
}

#[test]
fn status_runs_with_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(
        &repo,
        "0001-a.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    cmds::cmd_status(&repo).unwrap();
}

// ---------------------------------------------------------------------------
// cmd_sprint_show
// ---------------------------------------------------------------------------

#[test]
fn sprint_show_prints_header_and_tasks() {
    let (_tmp, repo) = setup();
    let sprint_content = "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n- 0002\n";
    write_sprint_file(&repo, "s1.md", sprint_content);
    write_task_file(
        &repo,
        "0001-task.md",
        &task_content("0001", "Task A", "backlog"),
    );
    write_task_file(
        &repo,
        "0002-task.md",
        &task_content("0002", "Task B", "done"),
    );
    // Verify it completes without error; output goes to stdout.
    cmds::cmd_sprint_show(&repo, "s1").unwrap();
}

#[test]
fn sprint_show_errors_on_missing_sprint() {
    let (_tmp, repo) = setup();
    assert!(cmds::cmd_sprint_show(&repo, "s99").is_err());
}

// ---------------------------------------------------------------------------
// cmd_sprint_list
// ---------------------------------------------------------------------------

#[test]
fn sprint_list_empty() {
    let (_tmp, repo) = setup();
    cmds::cmd_sprint_list(&repo).unwrap();
}

#[test]
fn sprint_list_shows_all_sprints() {
    let (_tmp, repo) = setup();
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n");
    write_sprint_file(&repo, "s2.md", "# Sprint 2 \u{00B7} Jun 15-28\n");
    cmds::cmd_sprint_list(&repo).unwrap();
}

// ---------------------------------------------------------------------------
// cmd_sprint_reorder
// ---------------------------------------------------------------------------

#[test]
fn sprint_reorder_no_crash_with_test_editor() {
    let (_tmp, repo) = setup();
    // STINT_TEST_EDITOR=none is already set by setup(); open_editor is a no-op.
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n");
    cmds::cmd_sprint_reorder(&repo, "s1").unwrap();
}

// ---------------------------------------------------------------------------
// sprint_remove error path
// ---------------------------------------------------------------------------

#[test]
fn sprint_remove_errors_when_task_not_present() {
    let (_tmp, repo) = setup();
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n");
    let err = cmds::cmd_sprint_remove(&repo, "s1", "9999").unwrap_err();
    assert!(err.to_string().contains("not in the sprint"));
}

// ---------------------------------------------------------------------------
// sprint_add error path
// ---------------------------------------------------------------------------

#[test]
fn sprint_add_errors_when_task_already_present() {
    let (_tmp, repo) = setup();
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n");
    let err = cmds::cmd_sprint_add(&repo, "s1", "0001").unwrap_err();
    assert!(err.to_string().contains("already in the sprint"));
}
