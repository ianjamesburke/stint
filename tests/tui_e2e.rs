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

fn write_task(repo: &StintRepo, id: &str, title: &str, status: &str) {
    write_task_extra(repo, id, title, status, "");
}

fn write_task_extra(repo: &StintRepo, id: &str, title: &str, status: &str, extra: &str) {
    let slug = title.to_lowercase().replace(' ', "-");
    let path = repo.tasks_dir().join(format!("{id}-{slug}.md"));
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!("{extra}\n")
    };
    fs::write(
        path,
        format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: {status}\n{extra}---\n"),
    )
    .unwrap();
}

fn insert_created_at(repo: &StintRepo, name: &str, timestamp: &str) {
    let path = repo.tasks_dir().join(name);
    let content = fs::read_to_string(&path).unwrap();
    let updated = content.replacen(
        "\nstatus: ",
        &format!("\ncreated_at: \"{}\"\nstatus: ", timestamp),
        1,
    );
    fs::write(path, updated).unwrap();
}

#[test]
fn renders_views_detail_navigation_and_quit() {
    let (_tmp, repo) = setup();
    replace_task_status(&repo, "0007-readme.md", "status: done", "status: archived");
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });

    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Runway"));
    assert!(screen.contains("build"));
    assert!(screen.contains("cli"));
    assert!(screen.contains("\u{25b6}0004 CLI argument"));
    assert!(screen.contains("0001 Project scaffold"));
    assert!(screen.contains("Parked 3"));
    assert!(screen.contains("blocked: 0001"));
    assert!(screen.contains("blocked: 0003, 0004"));
    // Backlog tasks stay off the runway entirely.
    assert!(!screen.contains("0006"));
    assert!(screen.contains("? shortcuts"));

    tui.press_char('?').unwrap();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Shortcuts"));
    assert!(screen.contains("Task actions"));
    assert!(screen.contains("c claim"));
    assert!(screen.contains("space/B backlog overlay"));
    tui.press_char('?').unwrap();
    assert!(!tui.render_text().unwrap().contains("Shortcuts"));

    // j/k move between lanes (parked is the last row), h/l move within a row.
    assert_eq!(tui.selected_task_id(), Some("0001"));
    tui.press_char('j').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0004"));
    tui.press_char('j').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0002"));
    tui.press_char('l').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0003"));
    tui.press_char('l').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0005"));
    tui.press_char('l').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0005"));
    press(&mut tui, KeyCode::Left);
    assert_eq!(tui.selected_task_id(), Some("0003"));
    tui.press_char('k').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0004"));

    press(&mut tui, KeyCode::Tab);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Table - sort id - done hidden"));
    assert!(screen.contains("Project scaffold"));
    press(&mut tui, KeyCode::Down);
    assert_eq!(tui.selected_task_id(), Some("0002"));
    press(&mut tui, KeyCode::Up);
    assert_eq!(tui.selected_task_id(), Some("0001"));

    press(&mut tui, KeyCode::BackTab);
    assert!(tui.render_text().unwrap().contains("Runway"));
    press(&mut tui, KeyCode::Tab);
    assert!(tui
        .render_text()
        .unwrap()
        .contains("Table - sort id - done hidden"));

    press(&mut tui, KeyCode::BackTab);
    assert_eq!(tui.selected_task_id(), Some("0001"));
    enter(&mut tui);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Detail - e editor - Esc close"));
    assert!(screen.contains("status: todo"));
    assert!(tui.commands_run().is_empty());
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: todo"));

    press(&mut tui, KeyCode::Esc);
    let screen = tui.render_text().unwrap();
    assert!(!screen.contains("Detail - e editor - Esc close"));

    tui.press_char('q').unwrap();
    assert!(tui.should_quit());
}

#[test]
fn blocked_todos_are_visible_in_parked_section() {
    let (_tmp, repo) = setup();
    let mut tui = driver(repo);

    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Parked 3"));
    assert!(screen.contains("0002"));
    assert!(screen.contains("0003"));
    assert!(screen.contains("0005"));
    assert!(screen.contains("blocked: 0001"));
}

#[test]
fn conflicted_ready_task_shows_holder_and_lane_goes_idle_when_freed() {
    let (_tmp, repo) = setup();
    write_task_extra(&repo, "0008", "Cli follow-up", "todo", "area:\n  - \"cli\"");
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });

    // 0004 (in-progress) holds the cli lane: 0008 is ready but waiting.
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("~0008 wait:0004"));
    assert_eq!(screen.matches("idle").count(), 1);

    replace_task_status(&repo, "0004-cli-args.md", "status: in-progress", "status: done");
    tui.reload();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("[0008 Cli follow-up"));
    assert!(!screen.contains("~0008"));
    assert_eq!(screen.matches("idle").count(), 2);
}

#[test]
fn unassigned_lane_collects_area_less_tasks() {
    let (_tmp, repo) = setup();
    write_task(&repo, "0008", "Free task", "todo");
    let mut tui = driver(repo);

    let screen = tui.render_text().unwrap();
    assert!(screen.contains("unassigned"));
    assert!(screen.contains("[0008 Free task"));
}

#[test]
fn backlog_overlay_lists_promotes_and_undoes() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });

    assert!(!tui.render_text().unwrap().contains("0006"));

    tui.press_char(' ').unwrap();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Backlog - r promote - Esc close"));
    assert!(screen.contains("0006 Cache API responses on disk"));

    tui.press_char('r').unwrap();
    assert!(task(&repo, "0006-caching-layer.md").contains("status: todo"));
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("backlog is empty"));

    press(&mut tui, KeyCode::Esc);
    // 0006 is blocked by 0003, so it lands in Parked once promoted.
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Parked 4"));
    assert!(screen.contains("0006"));

    tui.press_char('u').unwrap();
    assert!(task(&repo, "0006-caching-layer.md").contains("status: backlog"));
}

#[test]
fn enter_with_no_selected_task_shows_message_instead_of_empty_detail() {
    let (_tmp, repo) = setup();
    for name in [
        "0001-project-scaffold.md",
        "0002-config-loader.md",
        "0003-http-client.md",
        "0004-cli-args.md",
        "0005-render-forecast.md",
        "0006-caching-layer.md",
    ] {
        let path = repo.tasks_dir().join(name);
        let content = fs::read_to_string(&path).unwrap();
        let updated = content
            .replace("status: backlog", "status: done")
            .replace("status: todo", "status: done")
            .replace("status: in-progress", "status: done");
        fs::write(path, updated).unwrap();
    }
    let mut tui = driver(repo);

    enter(&mut tui);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("no task to select"));
    assert!(!screen.contains("Detail - e editor - Esc close"));
}

#[test]
fn table_scrolls_to_keep_selected_row_visible() {
    let (_tmp, repo) = setup();
    for i in 8..=28 {
        let id = format!("{i:04}");
        write_task(&repo, &id, &format!("Row {id}"), "todo");
    }
    let mut tui = TuiTestDriver::load(repo, 100, 12).unwrap();

    press(&mut tui, KeyCode::Tab);
    for _ in 0..15 {
        press(&mut tui, KeyCode::Down);
    }

    assert_eq!(tui.selected_task_id(), Some("0017"));
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("0017"));
    assert!(screen.contains("Row 0017"));
    assert!(!screen.contains("Project scaffold"));
}

#[test]
fn table_hides_done_by_default_and_toggles_them_visible() {
    let (_tmp, repo) = setup();
    let mut tui = driver(repo);

    press(&mut tui, KeyCode::Tab);
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Table - sort id - done hidden"));
    assert!(!screen.contains("Write the project README"));

    tui.press_char('D').unwrap();
    let screen = tui.render_text().unwrap();
    assert!(screen.contains("Table - sort id - done shown"));
    assert!(screen.contains("Write the project README"));
}

#[test]
fn external_reload_preserves_selection_and_reflects_disk_changes() {
    let (_tmp, repo) = setup();
    let mut tui = driver(StintRepo {
        stint_dir: repo.stint_dir.clone(),
    });
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
    assert_eq!(tui.selected_task_id(), Some("0001"));

    tui.press_char('c').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: in-progress"));
    assert!(content.contains("started_at:"));
    assert!(tui
        .render_text()
        .unwrap()
        .contains("claim command finished: 0001"));
    assert!(tui
        .commands_run()
        .single()
        .contains("claim-agent --id 0001"));
    tui.age_message();
    assert!(!tui
        .render_text()
        .unwrap()
        .contains("claim command finished: 0001"));

    tui.press_char('u').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: todo"));
    assert!(!content.contains("started_at:"));

    tui.press_ctrl('r').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: in-progress"));
    assert_eq!(tui.selected_task_id(), Some("0001"));

    tui.press_char('d').unwrap();
    let content = task(&repo, "0001-project-scaffold.md");
    assert!(content.contains("status: done"));
    assert!(content.contains("completed_at:"));

    // With 0001 done, selection falls to the cli lane's running task 0004.
    assert_eq!(tui.selected_task_id(), Some("0004"));
    tui.press_char('r').unwrap();
    assert!(task(&repo, "0004-cli-args.md").contains("status: todo"));

    assert_eq!(tui.selected_task_id(), Some("0004"));
    tui.press_char('b').unwrap();
    assert!(task(&repo, "0004-cli-args.md").contains("status: backlog"));

    // 0004 left the runway for the backlog; archive the new selection.
    assert_eq!(tui.selected_task_id(), Some("0002"));
    tui.press_char('a').unwrap();
    assert!(task(&repo, "0002-config-loader.md").contains("status: archived"));
}

#[test]
fn search_filter_and_sort_are_driven_by_prompts() {
    let (_tmp, repo) = setup();
    insert_created_at(&repo, "0001-project-scaffold.md", "2026-06-12T00:00:00Z");
    insert_created_at(&repo, "0002-config-loader.md", "2026-06-10T00:00:00Z");
    insert_created_at(&repo, "0003-http-client.md", "2026-06-11T00:00:00Z");
    let mut tui = driver(repo);
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
    assert!(tui.render_text().unwrap().contains("Table - sort created"));
    assert_eq!(tui.selected_task_id(), Some("0003"));
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
    let content = fs::read_to_string(&created).unwrap();
    assert!(content.contains("status: backlog"));
    assert!(content.contains("created_at:"));

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
    tui.press_char('l').unwrap();

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
    assert_eq!(tui.selected_task_id(), Some("0001"));

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

    // Selection drifted while 0001 was done; move back to the build lane.
    tui.press_char('k').unwrap();
    assert_eq!(tui.selected_task_id(), Some("0001"));

    tui.press_char('x').unwrap();
    tui.press_char('z').unwrap();
    assert!(task(&repo, "0001-project-scaffold.md").contains("status: in-progress"));

    let command = tui.commands_run().last().unwrap();
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
    tui.press_char('l').unwrap();
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
