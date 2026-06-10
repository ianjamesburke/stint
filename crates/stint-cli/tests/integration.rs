/// Integration tests for the stint-cli command layer.
///
/// Each test creates a temp dir, wires up a `StintRepo` pointing at a
/// `.stint/` sub-directory, then calls command functions directly.  No
/// subprocess invocation.
use std::env;
use std::fs;

use tempfile::TempDir;

// Bring the CLI modules into scope.  They live in `src/` so we use a path
// attribute to include them here.
#[path = "../src/repo.rs"]
mod repo;
#[path = "../src/cmds.rs"]
mod cmds;

use repo::StintRepo;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a `TempDir` with `.stint/tasks/` and `.stint/sprints/` pre-created.
fn setup() -> (TempDir, StintRepo) {
    // Suppress editor invocation in all tests.
    env::set_var("STINT_TEST_EDITOR", "none");

    let tmp = TempDir::new().unwrap();
    let stint_dir = tmp.path().join(".stint");
    fs::create_dir_all(stint_dir.join("tasks")).unwrap();
    fs::create_dir_all(stint_dir.join("sprints")).unwrap();
    let repo = StintRepo { stint_dir };
    (tmp, repo)
}

/// Write a raw task file into the repo's tasks dir.
fn write_task_file(repo: &StintRepo, filename: &str, content: &str) {
    fs::write(repo.tasks_dir().join(filename), content).unwrap();
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
    use stint_core::mutate::resolve_id;
    assert_eq!(resolve_id("0001"), "0001");
}

#[test]
fn resolve_id_partial() {
    use stint_core::mutate::resolve_id;
    assert_eq!(resolve_id("1"), "0001");
    assert_eq!(resolve_id("42"), "0042");
}

#[test]
fn resolve_id_with_slug() {
    use stint_core::mutate::resolve_id;
    assert_eq!(resolve_id("0001-auth-middleware"), "0001");
}

// ---------------------------------------------------------------------------
// cmd_add
// ---------------------------------------------------------------------------

#[test]
fn add_creates_file() {
    let (_tmp, repo) = setup();
    let path = cmds::cmd_add(&repo, "My first task").unwrap();
    assert!(path.exists());
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("id: \"0001\""));
    assert!(content.contains("My first task"));
    assert!(content.contains("status: backlog"));
}

#[test]
fn add_increments_id() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-existing.md", &task_content("0001", "Existing", "backlog"));
    let path = cmds::cmd_add(&repo, "Second task").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("id: \"0002\""));
}

#[test]
fn add_filename_includes_slug() {
    let (_tmp, repo) = setup();
    let path = cmds::cmd_add(&repo, "Auth Middleware").unwrap();
    let filename = path.file_name().unwrap().to_string_lossy();
    assert!(filename.starts_with("0001-auth-middleware"));
}

// ---------------------------------------------------------------------------
// cmd_list
// ---------------------------------------------------------------------------

#[test]
fn list_returns_all_tasks() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "backlog"));
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    let rows = cmds::cmd_list(&repo, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn list_filters_by_status() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "backlog"));
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    let rows = cmds::cmd_list(&repo, Some("backlog"), None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_filters_by_sprint() {
    let (_tmp, repo) = setup();
    let t1 = "---\nid: \"0001\"\ntitle: \"A\"\nstatus: todo\nsprint: \"s1\"\n---\n";
    let t2 = "---\nid: \"0002\"\ntitle: \"B\"\nstatus: todo\nsprint: \"s2\"\n---\n";
    write_task_file(&repo, "0001-a.md", t1);
    write_task_file(&repo, "0002-b.md", t2);
    let rows = cmds::cmd_list(&repo, None, Some("s1"), None, None).unwrap();
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
    let rows = cmds::cmd_list(&repo, None, None, Some("backend"), None).unwrap();
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
    let rows = cmds::cmd_list(&repo, None, None, None, Some("security")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "0001");
}

#[test]
fn list_empty_returns_empty_vec() {
    let (_tmp, repo) = setup();
    let rows = cmds::cmd_list(&repo, None, None, None, None).unwrap();
    assert!(rows.is_empty());
}

// ---------------------------------------------------------------------------
// cmd_done
// ---------------------------------------------------------------------------

#[test]
fn done_sets_status() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-task.md", &task_content("0001", "Task", "in-progress"));
    cmds::cmd_done(&repo, "0001", Some("2h")).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("status: done"));
}

#[test]
fn done_records_actual_time() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-task.md", &task_content("0001", "Task", "backlog"));
    cmds::cmd_done(&repo, "0001", Some("3h")).unwrap();
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
    cmds::cmd_done(&repo, "0001", None).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("actual: \"1h\""));
    assert!(content.contains("status: done"));
}

// ---------------------------------------------------------------------------
// cmd_log
// ---------------------------------------------------------------------------

#[test]
fn log_accumulates_time() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-task.md", &task_content("0001", "Task", "in-progress"));
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
    write_task_file(&repo, "0001-task.md", &task_content("0001", "Task", "backlog"));
    assert!(cmds::cmd_log(&repo, "0001", "2x").is_err());
}

// ---------------------------------------------------------------------------
// cmd_archive
// ---------------------------------------------------------------------------

#[test]
fn archive_sets_status() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-task.md", &task_content("0001", "Task", "backlog"));
    cmds::cmd_archive(&repo, "0001").unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("status: archived"));
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
fn sprint_remove_deletes_task() {
    let (_tmp, repo) = setup();
    write_sprint_file(&repo, "s1.md", "# Sprint 1 \u{00B7} Jun 1-14\n\n- 0001\n- 0002\n");
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
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "backlog"));
    write_task_file(&repo, "0002-b.md", &task_content("0002", "Task B", "done"));
    let errors = cmds::cmd_check(&repo).unwrap();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn check_detects_unresolved_blocked_by() {
    let (_tmp, repo) = setup();
    let content = "---\nid: \"0001\"\ntitle: \"T\"\nstatus: backlog\nblocked_by:\n  - \"9999\"\n---\n";
    write_task_file(&repo, "0001-t.md", content);
    let errors = cmds::cmd_check(&repo).unwrap();
    assert!(!errors.is_empty(), "expected at least one error");
    assert!(errors.iter().any(|e| e.contains("9999")));
}

#[test]
fn check_detects_duplicate_ids() {
    let (_tmp, repo) = setup();
    write_task_file(&repo, "0001-a.md", &task_content("0001", "Task A", "backlog"));
    // Different filename, same id field — triggers duplicate check.
    write_task_file(&repo, "0001-b.md", &task_content("0001", "Task B", "backlog"));
    let errors = cmds::cmd_check(&repo).unwrap();
    assert!(errors.iter().any(|e| e.contains("duplicate")));
}

#[test]
fn check_detects_id_filename_mismatch() {
    let (_tmp, repo) = setup();
    // id field says 0099 but filename prefix is 0001.
    let content = "---\nid: \"0099\"\ntitle: \"T\"\nstatus: backlog\n---\n";
    write_task_file(&repo, "0001-t.md", content);
    let errors = cmds::cmd_check(&repo).unwrap();
    assert!(errors.iter().any(|e| e.contains("filename")));
}

#[test]
fn check_empty_repo_passes() {
    let (_tmp, repo) = setup();
    let errors = cmds::cmd_check(&repo).unwrap();
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
    // Mark done — actual_override is ignored because actual is already set from the log.
    cmds::cmd_done(&repo, "0001", Some("2h")).unwrap();

    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(task.frontmatter.status, stint_core::schema::TaskStatus::Done);
    // actual stays at 1h (60m) — done does not overwrite an existing actual value.
    assert_eq!(
        task.frontmatter.actual,
        Some(stint_core::duration::Duration::from_minutes(60))
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
    cmds::cmd_done(&repo, "0001", Some("3h")).unwrap();
    let path = repo.resolve_task_path("0001").unwrap();
    let task = repo.read_task(&path).unwrap();
    assert_eq!(task.frontmatter.status, stint_core::schema::TaskStatus::Done);
    assert_eq!(
        task.frontmatter.actual,
        Some(stint_core::duration::Duration::from_minutes(180))
    );
}
