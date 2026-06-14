use std::fs;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use stint::repo::StintRepo;
use stint::tui::TuiTestDriver;
use tempfile::TempDir;

fn setup() -> (TempDir, StintRepo) {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        Path::new("tests/fixtures/tui/.stint"),
        &tmp.path().join(".stint"),
    );
    let repo = StintRepo {
        stint_dir: tmp.path().join(".stint"),
    };
    (tmp, repo)
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}

fn driver(repo: StintRepo) -> TuiTestDriver {
    TuiTestDriver::load(repo, 120, 32).unwrap()
}

fn press(driver: &mut TuiTestDriver, code: KeyCode) {
    driver
        .press(KeyEvent::new(code, KeyModifiers::NONE))
        .unwrap();
}

fn enter(driver: &mut TuiTestDriver) {
    press(driver, KeyCode::Enter);
}

fn task(repo: &StintRepo, name: &str) -> String {
    fs::read_to_string(repo.tasks_dir().join(name)).unwrap()
}

fn replace_task_status(repo: &StintRepo, name: &str, from: &str, to: &str) {
    let path = repo.tasks_dir().join(name);
    let content = fs::read_to_string(&path).unwrap().replace(from, to);
    fs::write(path, content).unwrap();
}

#[test]
fn renders_views_detail_navigation_and_quit() {
    let (_tmp, repo) = setup();
    let mut tui = driver(repo);

    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Dashboard"));
    assert!(screen.contains("CLI argument parsing"));
    assert!(screen.contains("? shortcuts"));
    assert!(!screen.contains("c claim - d done"));

    tui.press_char('?').unwrap();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Shortcuts"));
    assert!(screen.contains("Task actions"));
    assert!(screen.contains("c claim"));
    tui.press_char('?').unwrap();
    assert!(!tui.render_text().unwrap().contains("Shortcuts"));

    press(&mut tui, KeyCode::Tab);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("backlog"));
    assert!(screen.contains("ready"));
    assert!(screen.contains("blocked"));

    press(&mut tui, KeyCode::Right);
    assert_eq!(tui.selected_task_id(), Some("0001"));
    press(&mut tui, KeyCode::Left);
    assert_eq!(tui.selected_task_id(), Some("0006"));
    press(&mut tui, KeyCode::Right);
    assert_eq!(tui.selected_task_id(), Some("0001"));

    press(&mut tui, KeyCode::Tab);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Table - sort id"));
    assert!(screen.contains("Project scaffold"));
    press(&mut tui, KeyCode::Down);
    assert_eq!(tui.selected_task_id(), Some("0002"));
    press(&mut tui, KeyCode::Up);
    assert_eq!(tui.selected_task_id(), Some("0001"));

    press(&mut tui, KeyCode::Tab);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Sprint tasks"));
    assert!(screen.contains("exercise the TUI"));
    press(&mut tui, KeyCode::BackTab);
    assert!(tui.render_text().unwrap().contains("Table - sort id"));
    press(&mut tui, KeyCode::Tab);

    enter(&mut tui);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Detail - e editor - Esc close"));
    assert!(screen.contains("status: todo"));

    press(&mut tui, KeyCode::Esc);
    let screen = tui.render_text().unwrap();
    assert!(!screen.contains("Detail - e editor - Esc close"));

    tui.press_char('q').unwrap();
    assert!(tui.should_quit());
}

#[test]
fn enter_with_no_selected_task_shows_message_instead_of_empty_detail() {
    let (_tmp, repo) = setup();
    replace_task_status(
        &repo,
        "0004-cli-args.md",
        "status: in-progress",
        "status: done",
    );
    let mut tui = driver(repo);

    enter(&mut tui);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("no task to select"));
    assert!(!screen.contains("Detail - e editor - Esc close"));
}

#[test]
fn external_reload_preserves_selection_and_reflects_disk_changes() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Down);
    assert_eq!(tui.selected_task_id(), Some("0002"));

    let path = repo.tasks_dir().join("0002-config-loader.md");
    let content = fs::read_to_string(&path).unwrap().replace(
        "title: \"Config file loader\"",
        "title: \"Reloaded config loader\"",
    );
    fs::write(&path, content).unwrap();

    tui.reload();
    assert_eq!(tui.selected_task_id(), Some("0002"));
    assert!(tui
        .render_text()
        .unwrap()
        .contains("Reloaded config loader"));
}

#[test]
fn status_shortcuts_update_markdown_and_undo_redo() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Tab);
    assert_eq!(tui.selected_task_id(), Some("0001"));

    tui.press_char('c').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: in-progress"));
    assert!(content.contains("started_at:"));
    assert!(tui.render_text().unwrap().contains("claim: 0001"));
    tui.age_message();
    assert!(!tui.render_text().unwrap().contains("claim: 0001"));

    tui.press_char('u').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: todo"));
    assert!(!content.contains("started_at:"));

    tui.press_ctrl('r').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: in-progress"));

    tui.press_char('d').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: done"));
    assert!(content.contains("completed_at:"));

    tui.press_char('r').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: todo"));

    tui.press_char('b').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: backlog"));

    tui.press_char('a').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: archived"));
}

#[test]
fn search_filter_and_sort_are_driven_by_prompts() {
    let (_tmp, repo) = setup();
    let mut tui = driver(repo);
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Tab);

    tui.press_char('/').unwrap();
    tui.type_text("HTTP").unwrap();
    enter(&mut tui);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("HTTP client for the weather API"));
    assert!(!screen.contains("Config file loader"));

    tui.press_char('/').unwrap();
    for _ in 0..4 {
        press(&mut tui, KeyCode::Backspace);
    }
    enter(&mut tui);

    tui.press_char('f').unwrap();
    tui.type_text("network").unwrap();
    enter(&mut tui);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("HTTP client for the weather API"));
    assert!(screen.contains("Cache API responses on disk"));
    assert!(!screen.contains("Config file loader"));

    tui.press_char('s').unwrap();
    assert!(tui.render_text().unwrap().contains("Table - sort state"));
    tui.press_char('s').unwrap();
    assert!(tui.render_text().unwrap().contains("Table - sort sprint"));
}

#[test]
fn new_task_and_new_task_plus_edit_write_real_files() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });

    tui.press_char('n').unwrap();
    tui.type_text("Inbox triage").unwrap();
    enter(&mut tui);
    let created = repo.tasks_dir().join("0008-inbox-triage.md");
    assert!(created.is_file());
    assert!(fs::read_to_string(&created)
        .unwrap()
        .contains("status: backlog"));

    tui.set_editor_append("\nEdited body\n");
    tui.press_char('N').unwrap();
    tui.type_text("Edited task").unwrap();
    enter(&mut tui);
    let edited = repo.tasks_dir().join("0009-edited-task.md");
    assert!(edited.is_file());
    assert!(fs::read_to_string(&edited).unwrap().contains("Edited body"));
    assert_eq!(tui.edited_paths().len(), 1);
}

#[test]
fn editor_action_records_journal_and_undo_restores_file() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Tab);

    tui.set_editor_append("\nEdited from test\n");
    tui.press_char('e').unwrap();
    let path = repo.tasks_dir().join("0001-project-scaffold.md");
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("Edited from test"));
    assert_eq!(tui.edited_paths(), &[path.clone()]);

    tui.press_char('u').unwrap();
    assert!(!fs::read_to_string(&path)
        .unwrap()
        .contains("Edited from test"));
}

#[test]
fn command_palette_and_custom_commands_run_against_selected_task() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Tab);

    tui.press_char(':').unwrap();
    enter(&mut tui);
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: in-progress"));

    tui.press_char('u').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: todo"));

    tui.press_char(':').unwrap();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Search: type to filter commands"));
    tui.type_text("done").unwrap();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Search: done"));
    assert!(screen.contains("> mark selected task done"));
    assert!(!screen.contains("claim selected task"));
    enter(&mut tui);
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: done"));
    tui.press_char('u').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: todo"));

    tui.press_char('x').unwrap();
    tui.press_char('z').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: in-progress"));

    let command = tui.commands_run().single();
    assert!(command.contains("agent --id 0001"));
    assert!(command.contains("--slug project-scaffold"));
    assert!(command.contains("--path '"));
    assert!(command.contains("0001-project-scaffold.md'"));
    assert!(command.contains("--title 'Project scaffold'"));
    assert!(command.contains("--sprint s1"));
    assert!(command.contains("--estimate 1h"));
}

#[test]
fn custom_command_failure_is_reported_without_unclaiming() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });
    press(&mut tui, KeyCode::Tab);
    press(&mut tui, KeyCode::Tab);
    tui.set_command_success(false);

    tui.press_char('x').unwrap();
    tui.press_char('z').unwrap();

    assert!(task(&repo, "0001-project-scaffold.md").contains("status: in-progress"));
    assert!(tui
        .render_text()
        .unwrap()
        .contains("command failed: Agent on task"));
}

trait Single<T> {
    fn single(&self) -> &T;
}

impl<T> Single<T> for [T] {
    fn single(&self) -> &T {
        assert_eq!(self.len(), 1);
        &self[0]
    }
}
