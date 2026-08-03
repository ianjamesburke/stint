use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration as StdDuration, Instant};

use anyhow::{bail, Context};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
};
use ratatui::{Frame, Terminal};
use serde::Deserialize;
use stint::check::check;
use stint::mutate::{
    lower_priority, new_task_content, next_task_id, set_completed_at, set_started_at_if_absent,
    set_status, title_to_slug,
};
use stint::next::{compare_next_order, compute_next, focus_task, unblock_count, NextOptions};
use stint::schema::{BlockedByRef, Sprint, Task, TaskStatus};
use stint::serialize::serialize_task;
use stint::state::{active_blockers, classify, done_ids};
use stint::status::{compute_status, StatusReport};

use crate::repo::StintRepo;

type Term = Terminal<CrosstermBackend<Stdout>>;
const MESSAGE_TTL: StdDuration = StdDuration::from_secs(3);

pub fn run(repo: StintRepo) -> anyhow::Result<()> {
    let mut terminal = enter_terminal()?;
    let result = run_app(&mut terminal, repo);
    let restore = leave_terminal(&mut terminal);
    result.and(restore)
}

fn run_app(terminal: &mut Term, repo: StintRepo) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();
    let watcher = make_watcher(tx, repo.stint_dir.clone())?;
    let mut app = App::load(repo)?;

    loop {
        terminal.draw(|frame| app.render(frame))?;

        if app.should_quit {
            break;
        }

        if rx.try_iter().next().is_some() {
            app.reload_preserving_selection();
        }

        if event::poll(StdDuration::from_millis(150))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if app.handle_key(key, terminal)? {
                app.reload_preserving_selection();
            }
        }
    }

    drop(watcher);
    Ok(())
}

trait TerminalHost {
    fn edit_path(&mut self, path: &PathBuf) -> anyhow::Result<()>;
    fn run_command(&mut self, command: &str) -> anyhow::Result<bool>;
}

impl TerminalHost for Term {
    fn edit_path(&mut self, path: &PathBuf) -> anyhow::Result<()> {
        suspend_terminal(self, || open_editor_path(path))
    }

    fn run_command(&mut self, command: &str) -> anyhow::Result<bool> {
        suspend_terminal(self, || run_shell_command(command))
    }
}

fn make_watcher(tx: mpsc::Sender<()>, stint_dir: PathBuf) -> anyhow::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok() {
            let _ = tx.send(());
        }
    })
    .context("start .stint watcher")?;
    watcher
        .watch(&stint_dir, RecursiveMode::Recursive)
        .with_context(|| format!("watch {}", stint_dir.display()))?;
    Ok(watcher)
}

fn enter_terminal() -> anyhow::Result<Term> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("create terminal")
}

fn leave_terminal(terminal: &mut Term) -> anyhow::Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")
}

fn suspend_terminal<T>(
    terminal: &mut Term,
    f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")?;

    let result = f();

    enable_raw_mode().context("enable raw mode")?;
    execute!(terminal.backend_mut(), EnterAlternateScreen).context("enter alternate screen")?;
    terminal.clear().context("clear terminal")?;
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Focus,
    Runway,
    Table,
}

impl View {
    fn all() -> [Self; 3] {
        [Self::Focus, Self::Runway, Self::Table]
    }

    fn label(self) -> &'static str {
        match self {
            View::Focus => "Focus",
            View::Runway => "Runway",
            View::Table => "Table",
        }
    }

    fn next(self) -> Self {
        let views = Self::all();
        let pos = views.iter().position(|view| *view == self).unwrap_or(0);
        views[(pos + 1) % views.len()]
    }

    fn prev(self) -> Self {
        let views = Self::all();
        let pos = views.iter().position(|view| *view == self).unwrap_or(0);
        views[(pos + views.len() - 1) % views.len()]
    }
}

const CHIP_WIDTH: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunwayItemKind {
    Running,
    Ready,
    /// Ready by dependencies but its area is occupied; holders are the
    /// in-progress (or earlier-selected) task ids holding the area.
    Conflicted { holders: Vec<String> },
}

#[derive(Debug, Clone)]
struct RunwayItem {
    id: String,
    title: String,
    extra_areas: Vec<String>,
    kind: RunwayItemKind,
}

#[derive(Debug, Clone)]
struct RunwayLane {
    name: Option<String>,
    items: Vec<RunwayItem>,
}

impl RunwayLane {
    fn label(&self) -> &str {
        self.name.as_deref().unwrap_or("unassigned")
    }

    fn is_idle(&self) -> bool {
        !self
            .items
            .iter()
            .any(|item| item.kind == RunwayItemKind::Running)
    }
}

#[derive(Debug, Clone)]
struct ParkedItem {
    id: String,
    title: String,
    blockers: Vec<BlockedByRef>,
}

#[derive(Debug, Clone, Default)]
struct RunwayModel {
    lanes: Vec<RunwayLane>,
    parked: Vec<ParkedItem>,
}

impl RunwayModel {
    /// Row lengths in traversal order: one row per lane, plus a final row for
    /// the parked section when it is non-empty. Rows are never empty.
    fn row_lens(&self) -> Vec<usize> {
        let mut lens: Vec<usize> = self.lanes.iter().map(|lane| lane.items.len()).collect();
        if !self.parked.is_empty() {
            lens.push(self.parked.len());
        }
        lens
    }

    fn flat_ids(&self) -> Vec<&str> {
        self.lanes
            .iter()
            .flat_map(|lane| lane.items.iter().map(|item| item.id.as_str()))
            .chain(self.parked.iter().map(|item| item.id.as_str()))
            .collect()
    }

    fn locate(&self, flat: usize) -> Option<(usize, usize)> {
        let mut offset = 0;
        for (row, len) in self.row_lens().into_iter().enumerate() {
            if flat < offset + len {
                return Some((row, flat - offset));
            }
            offset += len;
        }
        None
    }

    fn flat_of(&self, row: usize, slot: usize) -> usize {
        self.row_lens().into_iter().take(row).sum::<usize>() + slot
    }
}

fn build_runway_model(
    tasks: &[Task],
    sprints: &[Sprint],
    search: &str,
    filter: &str,
) -> RunwayModel {
    let report = compute_next(
        tasks,
        sprints,
        NextOptions {
            sprint: None,
            include_area_conflicts: true,
            include_backlog: false,
        },
    );
    let done = done_ids(tasks);
    let task_sprint = task_sprint_map(sprints);
    let by_id: HashMap<&str, &Task> = tasks
        .iter()
        .map(|task| (task.frontmatter.id.as_str(), task))
        .collect();
    let search_needle = search.to_lowercase();
    let filter_needle = filter.to_lowercase();
    let keep = |id: &str| -> bool {
        let Some(task) = by_id.get(id) else {
            return false;
        };
        (search_needle.is_empty() || task_matches_text(task, &search_needle))
            && (filter_needle.is_empty()
                || task_matches_filter(task, &filter_needle, &task_sprint, &done))
    };

    let mut named: BTreeMap<String, Vec<RunwayItem>> = BTreeMap::new();
    let mut unassigned: Vec<RunwayItem> = Vec::new();
    let mut place = |first_area: Option<String>, item: RunwayItem| match first_area {
        Some(area) => named.entry(area).or_default().push(item),
        None => unassigned.push(item),
    };

    let mut running: Vec<&Task> = tasks
        .iter()
        .filter(|task| matches!(task.frontmatter.status, TaskStatus::InProgress))
        .collect();
    running.sort_by(|a, b| a.frontmatter.id.cmp(&b.frontmatter.id));
    for task in running {
        if !keep(&task.frontmatter.id) {
            continue;
        }
        let areas = &task.frontmatter.area;
        place(
            areas.first().cloned(),
            RunwayItem {
                id: task.frontmatter.id.clone(),
                title: task.frontmatter.title.clone(),
                extra_areas: areas.iter().skip(1).cloned().collect(),
                kind: RunwayItemKind::Running,
            },
        );
    }

    for task in &report.ready {
        if !keep(&task.id) {
            continue;
        }
        let mut holders: Vec<String> = task
            .area_conflicts
            .iter()
            .chain(task.selected_conflicts.iter())
            .cloned()
            .collect();
        holders.sort();
        holders.dedup();
        let kind = if holders.is_empty() {
            RunwayItemKind::Ready
        } else {
            RunwayItemKind::Conflicted { holders }
        };
        place(
            task.area.first().cloned(),
            RunwayItem {
                id: task.id.clone(),
                title: task.title.clone(),
                extra_areas: task.area.iter().skip(1).cloned().collect(),
                kind,
            },
        );
    }

    let mut lanes: Vec<RunwayLane> = named
        .into_iter()
        .map(|(name, items)| RunwayLane {
            name: Some(name),
            items,
        })
        .collect();
    if !unassigned.is_empty() {
        lanes.push(RunwayLane {
            name: None,
            items: unassigned,
        });
    }

    let parked = report
        .blocked
        .iter()
        .filter(|task| keep(&task.id))
        .map(|task| ParkedItem {
            id: task.id.clone(),
            title: task.title.clone(),
            blockers: task.blockers.clone(),
        })
        .collect();

    RunwayModel { lanes, parked }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Id,
    Created,
    State,
    Sprint,
    Priority,
}

impl SortMode {
    fn next(self) -> Self {
        match self {
            SortMode::Id => SortMode::Created,
            SortMode::Created => SortMode::State,
            SortMode::State => SortMode::Sprint,
            SortMode::Sprint => SortMode::Priority,
            SortMode::Priority => SortMode::Id,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortMode::Id => "id",
            SortMode::Created => "created",
            SortMode::State => "state",
            SortMode::Sprint => "sprint",
            SortMode::Priority => "priority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PromptKind {
    Search,
    Filter,
    NewTask { edit_after: bool },
    Command,
    DeleteConfirm { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandPaletteItem {
    Claim,
    Done,
    Ready,
    Defer,
    Archive,
    Edit,
    NewTask,
    FullNewTask,
    Undo,
    Redo,
}

impl CommandPaletteItem {
    fn all() -> [Self; 10] {
        [
            Self::Claim,
            Self::Done,
            Self::Ready,
            Self::Defer,
            Self::Archive,
            Self::Edit,
            Self::NewTask,
            Self::FullNewTask,
            Self::Undo,
            Self::Redo,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claim => "claim selected task",
            Self::Done => "mark selected task done",
            Self::Ready => "promote selected task to todo",
            Self::Defer => "defer selected task to backlog",
            Self::Archive => "archive selected task",
            Self::Edit => "open selected task in editor",
            Self::NewTask => "new task",
            Self::FullNewTask => "new task and edit",
            Self::Undo => "undo last TUI change",
            Self::Redo => "redo last undone change",
        }
    }
}

#[derive(Debug, Clone)]
struct CustomCommand {
    key: Option<char>,
    name: String,
    run: String,
    claim: bool,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    claim: Option<ClaimConfig>,
    #[serde(default)]
    command: Vec<CommandConfig>,
}

#[derive(Debug, Deserialize)]
struct ClaimConfig {
    run: String,
}

#[derive(Debug, Deserialize)]
struct CommandConfig {
    key: Option<String>,
    name: String,
    run: String,
    #[serde(default)]
    claim: bool,
}

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Debug, Clone)]
struct JournalEntry {
    label: String,
    files: Vec<FileSnapshot>,
}

#[derive(Debug)]
struct AppData {
    tasks: Vec<Task>,
    sprints: Vec<Sprint>,
    status: StatusReport,
    validation_errors: Vec<String>,
}

struct App {
    repo: StintRepo,
    data: AppData,
    view: View,
    selected: usize,
    sprint_index: usize,
    sort: SortMode,
    table_show_done: bool,
    search: String,
    filter: String,
    prompt: Option<PromptKind>,
    input: String,
    show_detail: bool,
    show_help: bool,
    show_backlog: bool,
    backlog_index: usize,
    command_index: usize,
    custom_menu: bool,
    custom_index: usize,
    claim_command: Option<String>,
    custom_commands: Vec<CustomCommand>,
    undo: Vec<JournalEntry>,
    redo: Vec<JournalEntry>,
    message: String,
    message_at: Instant,
    should_quit: bool,
}

impl App {
    fn load(repo: StintRepo) -> anyhow::Result<Self> {
        let data = load_data(&repo)?;
        let commands = load_commands(&repo);
        Ok(Self {
            repo,
            data,
            view: View::Runway,
            selected: 0,
            sprint_index: 0,
            sort: SortMode::Id,
            table_show_done: false,
            search: String::new(),
            filter: String::new(),
            prompt: None,
            input: String::new(),
            show_detail: false,
            show_help: false,
            show_backlog: false,
            backlog_index: 0,
            command_index: 0,
            custom_menu: false,
            custom_index: 0,
            claim_command: commands.claim,
            custom_commands: commands.custom,
            undo: Vec::new(),
            redo: Vec::new(),
            message: String::new(),
            message_at: Instant::now(),
            should_quit: false,
        })
    }

    fn reload_preserving_selection(&mut self) {
        let selected_id = self.selected_task_id().map(str::to_owned);
        match load_data(&self.repo) {
            Ok(data) => {
                self.data = data;
                let commands = load_commands(&self.repo);
                self.claim_command = commands.claim;
                self.custom_commands = commands.custom;
                self.restore_selection(selected_id.as_deref());
            }
            Err(error) => self.set_message(format!("reload failed: {error:#}")),
        }
    }

    fn restore_selection(&mut self, selected_id: Option<&str>) {
        let tasks = self.visible_tasks();
        if let Some(id) = selected_id {
            if let Some(pos) = tasks.iter().position(|task| task.frontmatter.id == id) {
                self.selected = pos;
                return;
            }
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let len = self.visible_tasks().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
        let sprint_count = self.data.sprints.len();
        if sprint_count == 0 {
            self.sprint_index = 0;
        } else if self.sprint_index >= sprint_count {
            self.sprint_index = sprint_count - 1;
        }
    }

    fn visible_tasks(&self) -> Vec<&Task> {
        match self.view {
            View::Focus => focus_task(&self.data.tasks, &self.data.sprints)
                .into_iter()
                .collect(),
            View::Runway => {
                let model = self.runway_model();
                let by_id: HashMap<&str, &Task> = self
                    .data
                    .tasks
                    .iter()
                    .map(|task| (task.frontmatter.id.as_str(), task))
                    .collect();
                model
                    .flat_ids()
                    .into_iter()
                    .filter_map(|id| by_id.get(id).copied())
                    .collect()
            }
            View::Table => {
                let done = done_ids(&self.data.tasks);
                let tasks: Vec<&Task> = self
                    .data
                    .tasks
                    .iter()
                    .filter(|task| self.table_show_done || !is_closed_task(task))
                    .collect();
                self.filtered_sorted_tasks(tasks, &done)
            }
        }
    }

    fn runway_model(&self) -> RunwayModel {
        build_runway_model(
            &self.data.tasks,
            &self.data.sprints,
            &self.search,
            &self.filter,
        )
    }

    fn backlog_tasks(&self) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self
            .data
            .tasks
            .iter()
            .filter(|task| matches!(task.frontmatter.status, TaskStatus::Backlog))
            .collect();
        tasks.sort_by(|a, b| compare_next_order(a, b));
        tasks
    }

    fn filtered_sorted_tasks<'a>(
        &self,
        mut tasks: Vec<&'a Task>,
        done: &HashSet<&str>,
    ) -> Vec<&'a Task> {
        if !self.search.is_empty() {
            let needle = self.search.to_lowercase();
            tasks.retain(|task| task_matches_text(task, &needle));
        }
        let task_sprint = task_sprint_map(&self.data.sprints);

        if !self.filter.is_empty() {
            let needle = self.filter.to_lowercase();
            tasks.retain(|task| task_matches_filter(task, &needle, &task_sprint, done));
        }

        match self.sort {
            SortMode::Id => tasks.sort_by(|a, b| a.frontmatter.id.cmp(&b.frontmatter.id)),
            SortMode::Created => tasks.sort_by(|a, b| compare_created_at(a, b)),
            SortMode::State => tasks.sort_by(|a, b| {
                classify(a, &done)
                    .as_str()
                    .cmp(classify(b, &done).as_str())
                    .then(a.frontmatter.id.cmp(&b.frontmatter.id))
            }),
            SortMode::Sprint => tasks.sort_by(|a, b| {
                task_sprint
                    .get(a.frontmatter.id.as_str())
                    .cmp(&task_sprint.get(b.frontmatter.id.as_str()))
                    .then(a.frontmatter.id.cmp(&b.frontmatter.id))
            }),
            SortMode::Priority => tasks.sort_by(|a, b| {
                stint::schema::cmp_priority(&a.frontmatter.priority, &b.frontmatter.priority)
                    .then(a.frontmatter.id.cmp(&b.frontmatter.id))
            }),
        }
        tasks
    }

    fn selected_task_id(&self) -> Option<&str> {
        self.visible_tasks()
            .get(self.selected)
            .map(|task| task.frontmatter.id.as_str())
    }

    fn selected_task(&self) -> Option<&Task> {
        self.visible_tasks().get(self.selected).copied()
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        self.clamp_selection();
        let area = frame.area();
        if self.view == View::Focus {
            self.render_focus(frame, area);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(2),
                ])
                .split(area);

            self.render_header(frame, chunks[0]);
            match self.view {
                View::Focus => unreachable!("focus renders without chrome"),
                View::Runway => self.render_runway(frame, chunks[1]),
                View::Table => self.render_table(frame, chunks[1]),
            }
            self.render_footer(frame, chunks[2]);
        }

        if self.show_detail {
            self.render_detail(frame, centered_rect(78, 82, area));
        }
        if self.show_help {
            self.render_help(frame, centered_rect(72, 84, area));
        }
        if self.prompt.is_some() {
            let prompt_area = if self.prompt == Some(PromptKind::Command) {
                centered_rect(62, 62, area)
            } else {
                prompt_rect(area)
            };
            self.render_prompt(frame, prompt_area);
        }
        if self.custom_menu {
            self.render_custom_menu(frame, centered_rect(64, 60, area));
        }
        if self.show_backlog {
            self.render_backlog_overlay(frame, centered_rect(64, 60, area));
        }
    }

    fn render_focus(&self, frame: &mut Frame<'_>, area: Rect) {
        let card_area = centered_rect(78, 64, area);
        let shortcuts_area = Rect {
            x: card_area.x,
            y: card_area
                .y
                .saturating_add(card_area.height)
                .saturating_add(1),
            width: card_area.width,
            height: 2.min(
                area.bottom()
                    .saturating_sub(card_area.bottom().saturating_add(1)),
            ),
        };
        let Some(task) = self.selected_task() else {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "No ready tasks. Everything is either complete, blocked, or already in progress.",
                    Style::default().fg(Color::Gray),
                ))
                .alignment(ratatui::layout::Alignment::Center),
                centered_rect(70, 12, area),
            );
            return;
        };
        let priority = task
            .frontmatter
            .priority
            .map(|priority| priority.as_str())
            .unwrap_or("unprioritized");
        let unblocks = unblock_count(task, &self.data.tasks);
        let card = Block::default()
            .borders(Borders::ALL)
            .title("Calculated next task");
        let content_area = card.inner(card_area);
        frame.render_widget(card, card_area);
        let content = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(content_area);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    "Focus now",
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::styled(
                    format!("{}  {}", task.frontmatter.id, task.frontmatter.title),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
            content[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(format!(
                "{priority} priority  ·  unblocks {unblocks} {}",
                if unblocks == 1 { "task" } else { "tasks" }
            )))
            .alignment(ratatui::layout::Alignment::Center),
            content[1],
        );
        frame.render_widget(
            Paragraph::new(task.body.as_str()).wrap(Wrap { trim: false }),
            content[2],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    "↓ lower priority    C claim    x delete",
                    Style::default().fg(Color::Gray),
                ),
                Line::styled(
                    "Tab other views    ? shortcuts",
                    Style::default().fg(Color::Gray),
                ),
            ])
            .alignment(ratatui::layout::Alignment::Center),
            shortcuts_area,
        );
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let titles = View::all()
            .into_iter()
            .map(|view| Line::from(view.label()))
            .collect::<Vec<_>>();
        let tabs = Tabs::new(titles)
            .select(
                View::all()
                    .iter()
                    .position(|view| *view == self.view)
                    .unwrap_or(0),
            )
            .block(Block::default().borders(Borders::ALL).title(format!(
                "stint open:{} backlog:{} errors:{}",
                self.data.status.open_count,
                self.data.status.backlog_count,
                self.data.validation_errors.len()
            )))
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, area);
    }

    fn render_runway(&self, frame: &mut Frame<'_>, area: Rect) {
        let model = self.runway_model();
        let parked_height = if model.parked.is_empty() {
            0
        } else {
            (model.parked.len() as u16 + 2).min(8)
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(parked_height)])
            .split(area);
        let selected_loc = model.locate(self.selected);
        self.render_runway_lanes(frame, chunks[0], &model, selected_loc);
        if !model.parked.is_empty() {
            self.render_parked(frame, chunks[1], &model, selected_loc);
        }
    }

    fn render_runway_lanes(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        model: &RunwayModel,
        selected_loc: Option<(usize, usize)>,
    ) {
        let title = format!("Runway {}", model.lanes.len());
        if model.lanes.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "no ready or in-progress tasks",
                    Style::default().fg(Color::Gray),
                ))
                .block(Block::default().borders(Borders::ALL).title(title)),
                area,
            );
            return;
        }
        let visible_rows = list_body_rows(area);
        let selected_lane = selected_loc
            .map(|(row, _)| row)
            .unwrap_or(0)
            .min(model.lanes.len() - 1);
        let start = viewport_start(selected_lane, model.lanes.len(), visible_rows);
        let label_width = model
            .lanes
            .iter()
            .map(|lane| lane.label().chars().count())
            .max()
            .unwrap_or(0)
            .max(10);
        let chip_capacity = usize::from(area.width)
            .saturating_sub(2 + label_width + 6)
            .checked_div(CHIP_WIDTH + 3)
            .unwrap_or(0)
            .max(1);
        let lines = model
            .lanes
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .map(|(lane_index, lane)| {
                let selected_slot = selected_loc
                    .filter(|(row, _)| *row == lane_index)
                    .map(|(_, slot)| slot);
                runway_lane_line(lane, selected_slot, label_width, chip_capacity)
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_bottom(runway_legend()),
            ),
            area,
        );
    }

    fn render_parked(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        model: &RunwayModel,
        selected_loc: Option<(usize, usize)>,
    ) {
        let parked_row = model.lanes.len();
        let selected_slot = selected_loc
            .filter(|(row, _)| *row == parked_row)
            .map(|(_, slot)| slot);
        let visible_rows = list_body_rows(area);
        let start = viewport_start(
            selected_slot.unwrap_or(0),
            model.parked.len(),
            visible_rows,
        );
        let items = model
            .parked
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .map(|(slot, item)| {
                let selected = selected_slot == Some(slot);
                let marker = if selected { "> " } else { "  " };
                let style = if selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Gray)),
                    Span::styled(
                        item.id.clone(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::raw(truncate(&item.title, 48)),
                    Span::styled(
                        format!(" - blocked: {}", format_blockers_inline(&item.blockers)),
                        Style::default().fg(Color::Yellow),
                    ),
                ]))
                .style(style)
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Parked {}", model.parked.len())),
            ),
            area,
        );
    }

    fn render_backlog_overlay(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        let tasks = self.backlog_tasks();
        let index = self.backlog_index.min(tasks.len().saturating_sub(1));
        let items = if tasks.is_empty() {
            vec![ListItem::new("backlog is empty")]
        } else {
            tasks
                .iter()
                .enumerate()
                .map(|(i, task)| {
                    let selected = i == index;
                    let marker = if selected { "> " } else { "  " };
                    let style = if selected {
                        Style::default().bg(Color::DarkGray)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, Style::default().fg(Color::Gray)),
                        Span::styled(
                            task.frontmatter.id.clone(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::raw(truncate(&task.frontmatter.title, 56)),
                    ]))
                    .style(style)
                })
                .collect()
        };
        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Backlog - r promote - Esc close"),
            ),
            area,
        );
    }

    fn render_table(&self, frame: &mut Frame<'_>, area: Rect) {
        let done = done_ids(&self.data.tasks);
        let tasks = self.visible_tasks();
        let visible_rows = table_body_rows(area);
        let start = viewport_start(self.selected, tasks.len(), visible_rows);
        let task_sprint: HashMap<&str, &str> = self
            .data
            .sprints
            .iter()
            .flat_map(|s| {
                s.task_ids
                    .iter()
                    .map(move |e| (numeric_prefix(e), s.header.id.as_str()))
            })
            .collect();
        let rows = tasks
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .map(|(index, task)| {
                let style = if index == self.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                let sprint_label = task_sprint
                    .get(task.frontmatter.id.as_str())
                    .copied()
                    .unwrap_or("-");
                Row::new(vec![
                    Cell::from(task.frontmatter.id.clone()),
                    Cell::from(classify(task, &done).as_str()),
                    Cell::from(task.frontmatter.created_at.as_deref().unwrap_or("-")),
                    Cell::from(
                        task.frontmatter
                            .estimate
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_owned()),
                    ),
                    Cell::from(sprint_label),
                    Cell::from(task.frontmatter.area.join(",")),
                    Cell::from(task.frontmatter.title.clone()),
                ])
                .style(style)
            })
            .collect::<Vec<_>>();
        let table = Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(10),
                Constraint::Length(20),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(16),
                Constraint::Min(16),
            ],
        )
        .header(
            Row::new(["ID", "STATE", "CREATED", "EST", "SPRINT", "AREA", "TITLE"])
                .style(Style::default().fg(Color::Gray)),
        )
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Table - sort {} - done {}",
            self.sort.label(),
            if self.table_show_done {
                "shown"
            } else {
                "hidden"
            }
        )));
        frame.render_widget(table, area);
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let lines = if self.message.is_empty() || self.message_at.elapsed() > MESSAGE_TTL {
            vec![
                Line::styled(
                    "tab views - hjkl move - enter detail - c claim - space backlog - ? shortcuts - q quit",
                    Style::default().fg(Color::Gray),
                ),
                Line::from(vec![
                    Span::styled(
                        format!(
                            "search:{}  filter:{}  sort:{}",
                            empty_dash(&self.search),
                            empty_dash(&self.filter),
                            self.sort.label()
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("undo:{} redo:{}", self.undo.len(), self.redo.len()),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        selected_summary(self.selected_task()),
                        Style::default().fg(Color::Gray),
                    ),
                ]),
            ]
        } else {
            vec![Line::styled(
                self.message.clone(),
                Style::default().fg(Color::Yellow),
            )]
        };
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
    }

    fn render_detail(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        let Some(task) = self.selected_task() else {
            return;
        };
        let done = done_ids(&self.data.tasks);
        let active = active_blockers(task, &done);
        let fm = &task.frontmatter;
        let sprint_label = self
            .data
            .sprints
            .iter()
            .find(|s| {
                s.task_ids
                    .iter()
                    .any(|e| numeric_prefix(e) == fm.id.as_str())
            })
            .map(|s| s.header.id.as_str())
            .unwrap_or("-");
        let mut lines = vec![
            Line::styled(
                format!("{} {}", fm.id, fm.title),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(format!("status: {}", fm.status)),
            Line::from(format!(
                "priority: {}",
                fm.priority.as_ref().map(|p| p.as_str()).unwrap_or("-")
            )),
            Line::from(format!(
                "size: {}",
                fm.size.as_ref().map(|s| s.as_str()).unwrap_or("-")
            )),
            Line::from(format!("state: {}", classify(task, &done).as_str())),
            Line::from(format!(
                "estimate: {}",
                fm.estimate
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_owned())
            )),
            Line::from(format!(
                "actual: {}",
                fm.actual
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_owned())
            )),
            Line::from(format!("sprint: {}", sprint_label)),
            Line::from(format!("area: {}", empty_dash(&fm.area.join(", ")))),
            Line::from(format!("tags: {}", empty_dash(&fm.tags.join(", ")))),
            Line::from(format!(
                "blocked_by: {}",
                if active.is_empty() {
                    "-".to_owned()
                } else {
                    format_blockers_inline(&active)
                }
            )),
            Line::from(""),
        ];
        lines.extend(task.body.lines().take(24).map(Line::from));

        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Detail - e editor - Esc close"),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_help(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        let lines = vec![
            Line::styled(
                "Navigation",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from("tab / shift-tab  switch views"),
            Line::from("j/k lanes        h/l within lane (runway)"),
            Line::from("arrows or hjkl    move selection"),
            Line::from("enter            open task detail"),
            Line::from("c                claim selected task"),
            Line::from("esc              close overlays"),
            Line::from("q / ctrl-c       quit"),
            Line::from(""),
            Line::styled(
                "Task actions",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from("c claim          d done          r ready"),
            Line::from("b defer          a archive       e edit"),
            Line::from("space/B backlog overlay - r promote"),
            Line::from("n new            N new + edit"),
            Line::from(""),
            Line::styled(
                "Tools",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from("/ search         f filter        s sort"),
            Line::from("D show done      x custom cmds    : command pal."),
            Line::from("u undo           ctrl-r redo      ? shortcuts"),
            Line::from(""),
            Line::styled(
                "Runway legend",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(vec![
                Span::styled(
                    "\u{25b6} green",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" running   "),
                Span::styled("cyan", Style::default().fg(Color::Cyan)),
                Span::raw(" ready   "),
                Span::styled("~yellow", Style::default().fg(Color::Yellow)),
                Span::raw(" area held by the wait: task"),
            ]),
            Line::from(vec![
                Span::raw("parked = blocked by blocked_by   "),
                Span::styled(
                    "idle",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" = lane free, work queued"),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("Shortcuts"))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_prompt(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        let title = match self.prompt {
            Some(PromptKind::Search) => "Search",
            Some(PromptKind::Filter) => "Filter",
            Some(PromptKind::NewTask { edit_after: false }) => "New task",
            Some(PromptKind::NewTask { edit_after: true }) => "New task + edit",
            Some(PromptKind::Command) => "Command palette",
            Some(PromptKind::DeleteConfirm { .. }) => "Delete task",
            None => "",
        };
        let lines = if self.prompt == Some(PromptKind::Command) {
            let items = self.palette_items();
            let mut lines = vec![Line::from(vec![
                Span::styled("Search: ", Style::default().fg(Color::Gray)),
                Span::raw(if self.input.is_empty() {
                    "type to filter commands"
                } else {
                    &self.input
                }),
            ])];
            lines.push(Line::from(""));
            if items.is_empty() {
                lines.push(Line::styled(
                    "No matching commands",
                    Style::default().fg(Color::Yellow),
                ));
            } else {
                lines.extend(items.iter().enumerate().map(|(index, item)| {
                    let marker = if index == self.command_index {
                        "> "
                    } else {
                        "  "
                    };
                    Line::from(format!("{marker}{}", item.label()))
                }));
            }
            lines
        } else if let Some(PromptKind::DeleteConfirm { id }) = &self.prompt {
            vec![
                Line::from(format!("type stint id ({id}) to confirm:")),
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(Color::Gray)),
                    Span::raw(&self.input),
                ]),
                Line::styled(
                    format!("stint remove {id}  ·  Esc cancels"),
                    Style::default().fg(Color::Yellow),
                ),
            ]
        } else {
            let label = match self.prompt {
                Some(PromptKind::Search) => "Search",
                Some(PromptKind::Filter) => "Filter",
                Some(PromptKind::NewTask { .. }) => "Title",
                _ => "Input",
            };
            vec![Line::from(vec![
                Span::styled(format!("{label}: "), Style::default().fg(Color::Gray)),
                Span::raw(&self.input),
            ])]
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(title))
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_custom_menu(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        let items = if self.custom_commands.is_empty() {
            vec![ListItem::new("no commands in .stint/config.toml")]
        } else {
            self.custom_commands
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    let prefix = if index == self.custom_index {
                        "> "
                    } else {
                        "  "
                    };
                    let key = command
                        .key
                        .map(|value| format!("[{}] ", value))
                        .unwrap_or_default();
                    ListItem::new(format!("{prefix}{key}{}", command.name))
                })
                .collect()
        };
        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Custom commands"),
            ),
            area,
        );
    }

    fn handle_key<H: TerminalHost>(
        &mut self,
        key: KeyEvent,
        terminal: &mut H,
    ) -> anyhow::Result<bool> {
        if self.custom_menu {
            return self.handle_custom_key(key, terminal);
        }
        if self.show_backlog {
            return self.handle_backlog_key(key);
        }
        if self.prompt.is_some() {
            return self.handle_prompt_key(key, terminal);
        }
        if self.show_help {
            match key.code {
                KeyCode::Esc => self.show_help = false,
                KeyCode::Char('?') if plain_key(key) => self.show_help = false,
                KeyCode::Char('q') if plain_key(key) => self.should_quit = true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.show_detail && key.code == KeyCode::Esc {
            self.show_detail = false;
            return Ok(false);
        }

        if self.view == View::Focus {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') if plain_key(key) => {
                    return self.lower_selected_priority()
                }
                KeyCode::Char('C') if plain_key(key) => return self.claim_selected(terminal),
                KeyCode::Char('x') if plain_key(key) => {
                    let Some(id) = self.selected_task_id() else {
                        self.set_message("no task to delete".to_owned());
                        return Ok(false);
                    };
                    self.open_prompt(PromptKind::DeleteConfirm { id: id.to_owned() }, "");
                    return Ok(false);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true
            }
            KeyCode::Char('q') if plain_key(key) => self.should_quit = true,
            KeyCode::Tab => self.switch_view(self.view.next()),
            KeyCode::BackTab => self.switch_view(self.view.prev()),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Char('j') if plain_key(key) => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('k') if plain_key(key) => self.move_selection(-1),
            KeyCode::Right => self.move_horizontal(1),
            KeyCode::Char('l') if plain_key(key) => self.move_horizontal(1),
            KeyCode::Left => self.move_horizontal(-1),
            KeyCode::Char('h') if plain_key(key) => self.move_horizontal(-1),
            KeyCode::Enter => {
                if self.selected_task().is_some() {
                    self.show_detail = true;
                } else {
                    self.set_message("no task to select".to_owned());
                }
            }
            KeyCode::Char('e') if plain_key(key) => self.edit_selected(terminal)?,
            KeyCode::Char('c') if plain_key(key) => return self.claim_selected(terminal),
            KeyCode::Char('d') if plain_key(key) => return self.transition_done(),
            KeyCode::Char('D') if plain_key(key) => {
                self.table_show_done = !self.table_show_done;
                self.selected = 0;
                self.set_message(format!(
                    "done: {}",
                    if self.table_show_done {
                        "shown"
                    } else {
                        "hidden"
                    }
                ));
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return self.redo()
            }
            KeyCode::Char('r') if plain_key(key) => {
                return self.transition_selected("ready", TaskStatus::Todo, false)
            }
            KeyCode::Char('b') if plain_key(key) => {
                return self.transition_selected("defer", TaskStatus::Backlog, false)
            }
            KeyCode::Char(' ') | KeyCode::Char('B') if plain_key(key) => {
                self.show_backlog = true;
                self.backlog_index = 0;
            }
            KeyCode::Char('a') if plain_key(key) => {
                return self.transition_selected("archive", TaskStatus::Archived, false)
            }
            KeyCode::Char('n') if plain_key(key) => {
                self.open_prompt(PromptKind::NewTask { edit_after: false }, "")
            }
            KeyCode::Char('N') if plain_key(key) => {
                self.open_prompt(PromptKind::NewTask { edit_after: true }, "")
            }
            KeyCode::Char('/') if plain_key(key) => {
                self.open_prompt(PromptKind::Search, &self.search.clone())
            }
            KeyCode::Char('f') if plain_key(key) => {
                self.open_prompt(PromptKind::Filter, &self.filter.clone())
            }
            KeyCode::Char('s') if plain_key(key) => {
                if self.view == View::Runway {
                    self.set_message("sort applies to table view".to_owned());
                } else {
                    self.sort = self.sort.next();
                    self.set_message(format!("sort: {}", self.sort.label()));
                }
            }
            KeyCode::Char('x') if plain_key(key) => self.custom_menu = true,
            KeyCode::Char(':') if plain_key(key) => self.open_prompt(PromptKind::Command, ""),
            KeyCode::Char('?') if plain_key(key) => self.show_help = !self.show_help,
            KeyCode::Char('u') if plain_key(key) => return self.undo(),
            _ => {}
        }
        Ok(false)
    }

    fn handle_prompt_key<H: TerminalHost>(
        &mut self,
        key: KeyEvent,
        terminal: &mut H,
    ) -> anyhow::Result<bool> {
        let Some(prompt) = self.prompt.clone() else {
            return Ok(false);
        };
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.input.clear();
            }
            KeyCode::Enter => {
                let value = self.input.trim().to_owned();
                match prompt {
                    PromptKind::DeleteConfirm { id } => {
                        if value == id {
                            self.prompt = None;
                            self.input.clear();
                            return self.delete_task(&id);
                        }
                        self.input.clear();
                        self.set_message(format!("confirmation must match {id}"));
                    }
                    PromptKind::Search => {
                        self.prompt = None;
                        self.input.clear();
                        self.search = value;
                        self.selected = 0;
                    }
                    PromptKind::Filter => {
                        self.prompt = None;
                        self.input.clear();
                        self.filter = value;
                        self.selected = 0;
                    }
                    PromptKind::NewTask { edit_after } => {
                        self.prompt = None;
                        self.input.clear();
                        if !value.is_empty() {
                            return self.create_task(&value, edit_after, terminal);
                        }
                    }
                    PromptKind::Command => {
                        self.prompt = None;
                        let result = self.run_palette_item(terminal);
                        if self.prompt.is_none() {
                            self.input.clear();
                        }
                        return result;
                    }
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                if prompt == PromptKind::Command {
                    self.command_index = 0;
                }
            }
            KeyCode::Down if prompt == PromptKind::Command => self.move_palette_selection(1),
            KeyCode::Char('j') if prompt == PromptKind::Command && plain_key(key) => {
                self.move_palette_selection(1);
            }
            KeyCode::Up if prompt == PromptKind::Command => self.move_palette_selection(-1),
            KeyCode::Char('k') if prompt == PromptKind::Command && plain_key(key) => {
                self.move_palette_selection(-1);
            }
            KeyCode::Char(ch) if plain_key(key) => {
                self.input.push(ch);
                if prompt == PromptKind::Command {
                    self.command_index = 0;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_custom_key<H: TerminalHost>(
        &mut self,
        key: KeyEvent,
        terminal: &mut H,
    ) -> anyhow::Result<bool> {
        match key.code {
            KeyCode::Esc => self.custom_menu = false,
            KeyCode::Char('x') if plain_key(key) => self.custom_menu = false,
            KeyCode::Down => {
                if !self.custom_commands.is_empty() {
                    self.custom_index = (self.custom_index + 1) % self.custom_commands.len();
                }
            }
            KeyCode::Char('j') if plain_key(key) => {
                if !self.custom_commands.is_empty() {
                    self.custom_index = (self.custom_index + 1) % self.custom_commands.len();
                }
            }
            KeyCode::Up => {
                if !self.custom_commands.is_empty() {
                    let len = self.custom_commands.len();
                    self.custom_index = (self.custom_index + len - 1) % len;
                }
            }
            KeyCode::Char('k') if plain_key(key) => {
                if !self.custom_commands.is_empty() {
                    let len = self.custom_commands.len();
                    self.custom_index = (self.custom_index + len - 1) % len;
                }
            }
            KeyCode::Enter => return self.run_custom_command(terminal),
            KeyCode::Char(ch) if plain_key(key) => {
                if let Some(index) = self
                    .custom_commands
                    .iter()
                    .position(|command| command.key == Some(ch))
                {
                    self.custom_index = index;
                    return self.run_custom_command(terminal);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn run_palette_item<H: TerminalHost>(&mut self, terminal: &mut H) -> anyhow::Result<bool> {
        let items = self.palette_items();
        let Some(item) = items.get(self.command_index).copied() else {
            self.set_message("no matching command".to_owned());
            return Ok(false);
        };
        match item {
            CommandPaletteItem::Claim => self.claim_selected(terminal),
            CommandPaletteItem::Done => self.transition_done(),
            CommandPaletteItem::Ready => self.transition_selected("ready", TaskStatus::Todo, false),
            CommandPaletteItem::Defer => {
                self.transition_selected("defer", TaskStatus::Backlog, false)
            }
            CommandPaletteItem::Archive => {
                self.transition_selected("archive", TaskStatus::Archived, false)
            }
            CommandPaletteItem::Edit => {
                self.edit_selected(terminal)?;
                Ok(true)
            }
            CommandPaletteItem::NewTask => {
                self.open_prompt(PromptKind::NewTask { edit_after: false }, "");
                Ok(false)
            }
            CommandPaletteItem::FullNewTask => {
                self.open_prompt(PromptKind::NewTask { edit_after: true }, "");
                Ok(false)
            }
            CommandPaletteItem::Undo => self.undo(),
            CommandPaletteItem::Redo => self.redo(),
        }
    }

    fn switch_view(&mut self, view: View) {
        self.view = view;
        self.search.clear();
        self.filter.clear();
        self.selected = 0;
        self.show_detail = false;
    }

    fn move_selection(&mut self, delta: isize) {
        match self.view {
            View::Focus => {}
            View::Runway => self.move_runway_lane(delta),
            View::Table => self.move_linear(delta),
        }
    }

    fn move_horizontal(&mut self, delta: isize) {
        match self.view {
            View::Focus => {}
            View::Runway => self.move_runway_slot(delta),
            View::Table => self.move_linear(delta),
        }
    }

    fn move_linear(&mut self, delta: isize) {
        let len = self.visible_tasks().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = wrap_index(self.selected, len, delta);
    }

    fn move_runway_slot(&mut self, delta: isize) {
        let model = self.runway_model();
        let Some((row, slot)) = model.locate(self.selected) else {
            return;
        };
        let len = model.row_lens()[row];
        let new_slot = if delta.is_negative() {
            slot.saturating_sub(delta.unsigned_abs())
        } else {
            (slot + delta as usize).min(len - 1)
        };
        self.selected = model.flat_of(row, new_slot);
    }

    fn move_runway_lane(&mut self, delta: isize) {
        let model = self.runway_model();
        let lens = model.row_lens();
        if lens.is_empty() {
            self.selected = 0;
            return;
        }
        let (row, slot) = model.locate(self.selected).unwrap_or((0, 0));
        let new_row = wrap_index(row, lens.len(), delta);
        let new_slot = slot.min(lens[new_row] - 1);
        self.selected = model.flat_of(new_row, new_slot);
    }

    fn handle_backlog_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
        let len = self.backlog_tasks().len();
        self.backlog_index = self.backlog_index.min(len.saturating_sub(1));
        match key.code {
            KeyCode::Esc => self.show_backlog = false,
            KeyCode::Char(' ') | KeyCode::Char('B') if plain_key(key) => self.show_backlog = false,
            KeyCode::Char('q') if plain_key(key) => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true
            }
            KeyCode::Down => self.backlog_index = wrap_index(self.backlog_index, len, 1),
            KeyCode::Char('j') if plain_key(key) => {
                self.backlog_index = wrap_index(self.backlog_index, len, 1)
            }
            KeyCode::Up => self.backlog_index = wrap_index(self.backlog_index, len, -1),
            KeyCode::Char('k') if plain_key(key) => {
                self.backlog_index = wrap_index(self.backlog_index, len, -1)
            }
            KeyCode::Enter => return self.promote_backlog_selection(),
            KeyCode::Char('r') if plain_key(key) => return self.promote_backlog_selection(),
            _ => {}
        }
        Ok(false)
    }

    fn promote_backlog_selection(&mut self) -> anyhow::Result<bool> {
        let id = {
            let tasks = self.backlog_tasks();
            let Some(task) = tasks.get(self.backlog_index) else {
                self.set_message("backlog is empty".to_owned());
                return Ok(false);
            };
            task.frontmatter.id.clone()
        };
        self.transition_task_by_id(&id, "ready", TaskStatus::Todo, false)
    }

    fn open_prompt(&mut self, prompt: PromptKind, value: &str) {
        let is_command = prompt == PromptKind::Command;
        self.prompt = Some(prompt);
        self.input = value.to_owned();
        if is_command {
            self.command_index = 0;
        }
    }

    fn palette_items(&self) -> Vec<CommandPaletteItem> {
        let needle = self.input.to_lowercase();
        CommandPaletteItem::all()
            .into_iter()
            .filter(|item| needle.is_empty() || item.label().contains(&needle))
            .collect()
    }

    fn move_palette_selection(&mut self, delta: isize) {
        self.command_index = wrap_index(self.command_index, self.palette_items().len(), delta);
    }

    fn edit_selected<H: TerminalHost>(&mut self, terminal: &mut H) -> anyhow::Result<()> {
        let id = self
            .selected_task_id()
            .ok_or_else(|| anyhow::anyhow!("no selected task"))?
            .to_owned();
        let path = self.repo.resolve_task_path(&id)?;
        let before = read_optional(&path)?;
        terminal.edit_path(&path)?;
        let after = read_optional(&path)?;
        if before != after {
            self.push_journal(
                "edit",
                vec![FileSnapshot {
                    path,
                    before,
                    after,
                }],
            );
        }
        Ok(())
    }

    fn lower_selected_priority(&mut self) -> anyhow::Result<bool> {
        let id = self
            .selected_task_id()
            .ok_or_else(|| anyhow::anyhow!("no selected task"))?
            .to_owned();
        let path = self.repo.resolve_task_path(&id)?;
        let before = read_optional(&path)?;
        let mut task = self.repo.read_task(&path)?;
        if !lower_priority(&mut task) {
            self.set_message(format!("already unprioritized: {id}"));
            return Ok(false);
        }
        let after_content = serialize_task(&task);
        self.repo.write_task(&path, &after_content)?;
        self.push_journal(
            "lower priority",
            vec![FileSnapshot {
                path,
                before,
                after: Some(after_content),
            }],
        );
        self.set_message(format!("priority lowered: {id}"));
        Ok(true)
    }

    fn delete_task(&mut self, id: &str) -> anyhow::Result<bool> {
        let path = self.repo.resolve_task_path(id)?;
        let before = read_optional(&path)?;
        fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))?;
        self.push_journal(
            "delete",
            vec![FileSnapshot {
                path,
                before,
                after: None,
            }],
        );
        self.set_message(format!("deleted: {id}"));
        Ok(true)
    }

    fn claim_selected<H: TerminalHost>(&mut self, terminal: &mut H) -> anyhow::Result<bool> {
        let Some(task) = self.selected_task().cloned() else {
            self.set_message("no selected task".to_owned());
            return Ok(false);
        };

        let changed = if matches!(task.frontmatter.status, TaskStatus::InProgress) {
            false
        } else {
            self.transition_selected("claim", TaskStatus::InProgress, true)?
        };

        if self.claim_command.is_some() {
            self.reload_preserving_selection();
            let task = self
                .selected_task()
                .cloned()
                .filter(|current| current.frontmatter.id == task.frontmatter.id)
                .unwrap_or(task);
            let path = self.repo.resolve_task_path(&task.frontmatter.id)?;
            let run = expand_command(
                self.claim_command.as_deref().unwrap_or_default(),
                &task,
                &path,
                &self.data.sprints,
            );
            let status = terminal.run_command(&run)?;
            if status {
                self.set_message(format!("claim command finished: {}", task.frontmatter.id));
            } else {
                self.set_message(format!("claim command failed: {}", task.frontmatter.id));
            }
            return Ok(true);
        }

        Ok(changed)
    }

    fn transition_selected(
        &mut self,
        label: &str,
        status: TaskStatus,
        set_started: bool,
    ) -> anyhow::Result<bool> {
        let id = self
            .selected_task_id()
            .ok_or_else(|| anyhow::anyhow!("no selected task"))?
            .to_owned();
        self.transition_task_by_id(&id, label, status, set_started)
    }

    fn transition_task_by_id(
        &mut self,
        id: &str,
        label: &str,
        status: TaskStatus,
        set_started: bool,
    ) -> anyhow::Result<bool> {
        let path = self.repo.resolve_task_path(id)?;
        let before = read_optional(&path)?;
        let mut task = self.repo.read_task(&path)?;
        set_status(&mut task, status);
        if set_started {
            set_started_at_if_absent(&mut task, now_timestamp());
        }
        let after_content = serialize_task(&task);
        self.repo.write_task(&path, &after_content)?;
        self.push_journal(
            label,
            vec![FileSnapshot {
                path,
                before,
                after: Some(after_content),
            }],
        );
        self.set_message(format!("{label}: {id}"));
        Ok(true)
    }

    fn transition_done(&mut self) -> anyhow::Result<bool> {
        let id = self
            .selected_task_id()
            .ok_or_else(|| anyhow::anyhow!("no selected task"))?
            .to_owned();
        let path = self.repo.resolve_task_path(&id)?;
        let before = read_optional(&path)?;
        let mut task = self.repo.read_task(&path)?;
        set_status(&mut task, TaskStatus::Done);
        set_completed_at(&mut task, now_timestamp());
        let after_content = serialize_task(&task);
        self.repo.write_task(&path, &after_content)?;
        self.push_journal(
            "done",
            vec![FileSnapshot {
                path,
                before,
                after: Some(after_content),
            }],
        );
        self.set_message(format!("done: {id}"));
        Ok(true)
    }

    fn create_task<H: TerminalHost>(
        &mut self,
        title: &str,
        edit_after: bool,
        terminal: &mut H,
    ) -> anyhow::Result<bool> {
        self.repo.ensure_dirs()?;
        let tasks = self.repo.load_tasks()?;
        let id = next_task_id(&tasks);
        let slug = title_to_slug(title);
        let filename = if slug.is_empty() {
            format!("{id}.md")
        } else {
            format!("{id}-{slug}.md")
        };
        let path = self.repo.tasks_dir().join(filename);
        let created_at = now_timestamp();
        let content = new_task_content(&id, title, Some(&created_at));
        self.repo.write_task(&path, &content)?;
        if edit_after {
            terminal.edit_path(&path)?;
        }
        let after = read_optional(&path)?;
        self.push_journal(
            "new task",
            vec![FileSnapshot {
                path,
                before: None,
                after,
            }],
        );
        self.set_message(format!("created: {id}"));
        Ok(true)
    }

    fn run_custom_command<H: TerminalHost>(&mut self, terminal: &mut H) -> anyhow::Result<bool> {
        self.custom_menu = false;
        if self.custom_commands.is_empty() {
            return Ok(false);
        }
        let command = self.custom_commands[self.custom_index].clone();
        let Some(task) = self.selected_task().cloned() else {
            self.set_message("no selected task".to_owned());
            return Ok(false);
        };

        if command.claim && !matches!(task.frontmatter.status, TaskStatus::InProgress) {
            self.transition_selected("claim", TaskStatus::InProgress, true)?;
            self.reload_preserving_selection();
        }

        let path = self.repo.resolve_task_path(&task.frontmatter.id)?;
        let run = expand_command(&command.run, &task, &path, &self.data.sprints);
        let status = terminal.run_command(&run)?;
        if status {
            self.set_message(format!("command finished: {}", command.name));
        } else {
            self.set_message(format!("command failed: {}", command.name));
        }
        Ok(true)
    }

    fn undo(&mut self) -> anyhow::Result<bool> {
        let Some(entry) = self.undo.pop() else {
            self.set_message("nothing to undo".to_owned());
            return Ok(false);
        };
        apply_snapshots(&entry.files, SnapshotSide::Before)?;
        self.set_message(format!("undid {}", entry.label));
        self.redo.push(entry);
        Ok(true)
    }

    fn redo(&mut self) -> anyhow::Result<bool> {
        let Some(entry) = self.redo.pop() else {
            self.set_message("nothing to redo".to_owned());
            return Ok(false);
        };
        apply_snapshots(&entry.files, SnapshotSide::After)?;
        self.set_message(format!("redid {}", entry.label));
        self.undo.push(entry);
        Ok(true)
    }

    fn push_journal(&mut self, label: &str, files: Vec<FileSnapshot>) {
        self.undo.push(JournalEntry {
            label: label.to_owned(),
            files,
        });
        self.redo.clear();
    }

    fn set_message(&mut self, message: String) {
        self.message = message;
        self.message_at = Instant::now();
    }
}

#[doc(hidden)]
pub struct TuiTestDriver {
    app: App,
    host: TestHost,
    terminal: Terminal<TestBackend>,
}

impl TuiTestDriver {
    pub fn load(repo: StintRepo, width: u16, height: u16) -> anyhow::Result<Self> {
        Ok(Self {
            app: App::load(repo)?,
            host: TestHost::default(),
            terminal: Terminal::new(TestBackend::new(width, height))?,
        })
    }

    pub fn render_text(&mut self) -> anyhow::Result<String> {
        self.terminal.draw(|frame| self.app.render(frame))?;
        Ok(buffer_text(self.terminal.backend().buffer()))
    }

    pub fn press(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        if self.app.handle_key(key, &mut self.host)? {
            self.app.reload_preserving_selection();
        }
        Ok(())
    }

    pub fn press_char(&mut self, ch: char) -> anyhow::Result<()> {
        self.press(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
    }

    pub fn press_ctrl(&mut self, ch: char) -> anyhow::Result<()> {
        self.press(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
    }

    pub fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for ch in text.chars() {
            self.press_char(ch)?;
        }
        Ok(())
    }

    pub fn reload(&mut self) {
        self.app.reload_preserving_selection();
    }

    pub fn selected_task_id(&self) -> Option<&str> {
        self.app.selected_task_id()
    }

    pub fn should_quit(&self) -> bool {
        self.app.should_quit
    }

    pub fn set_editor_append(&mut self, append: impl Into<String>) {
        self.host.editor_append = append.into();
    }

    pub fn set_command_success(&mut self, success: bool) {
        self.host.command_success = success;
    }

    pub fn edited_paths(&self) -> &[PathBuf] {
        &self.host.edited_paths
    }

    pub fn commands_run(&self) -> &[String] {
        &self.host.commands_run
    }

    pub fn age_message(&mut self) {
        self.app.message_at = Instant::now() - MESSAGE_TTL - StdDuration::from_millis(1);
    }
}

struct TestHost {
    edited_paths: Vec<PathBuf>,
    commands_run: Vec<String>,
    editor_append: String,
    command_success: bool,
}

impl Default for TestHost {
    fn default() -> Self {
        Self {
            edited_paths: Vec::new(),
            commands_run: Vec::new(),
            editor_append: String::new(),
            command_success: true,
        }
    }
}

impl TerminalHost for TestHost {
    fn edit_path(&mut self, path: &PathBuf) -> anyhow::Result<()> {
        self.edited_paths.push(path.clone());
        if !self.editor_append.is_empty() {
            let mut content =
                fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
            content.push_str(&self.editor_append);
            fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
        }
        Ok(())
    }

    fn run_command(&mut self, command: &str) -> anyhow::Result<bool> {
        self.commands_run.push(command.to_owned());
        Ok(self.command_success)
    }
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn load_data(repo: &StintRepo) -> anyhow::Result<AppData> {
    let tasks = repo.load_tasks()?;
    let sprints = repo.load_sprints()?;
    let status = compute_status(&tasks, &sprints, None);
    let validation_errors = check(&tasks, &sprints)
        .into_iter()
        .map(|error| error.to_string())
        .collect();
    Ok(AppData {
        tasks,
        sprints,
        status,
        validation_errors,
    })
}

struct LoadedCommands {
    claim: Option<String>,
    custom: Vec<CustomCommand>,
}

fn load_commands(repo: &StintRepo) -> LoadedCommands {
    let path = repo.stint_dir.join("config.toml");
    let Ok(content) = fs::read_to_string(path) else {
        return LoadedCommands {
            claim: None,
            custom: Vec::new(),
        };
    };
    let Ok(config) = toml::from_str::<ConfigFile>(&content) else {
        return LoadedCommands {
            claim: None,
            custom: Vec::new(),
        };
    };
    let custom = config
        .command
        .into_iter()
        .map(|command| CustomCommand {
            key: command.key.and_then(|key| key.chars().next()),
            name: command.name,
            run: command.run,
            claim: command.claim,
        })
        .collect();
    LoadedCommands {
        claim: config.claim.map(|claim| claim.run),
        custom,
    }
}

fn runway_legend() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            " \u{25b6} running ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("ready ", Style::default().fg(Color::Cyan)),
        Span::styled("~waiting on holder ", Style::default().fg(Color::Yellow)),
        Span::styled("parked = blocked ", Style::default().fg(Color::Gray)),
    ])
}

fn runway_lane_line(
    lane: &RunwayLane,
    selected_slot: Option<usize>,
    label_width: usize,
    chip_capacity: usize,
) -> Line<'static> {
    let idle = lane.is_idle();
    let label_style = if idle {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let mut spans = vec![
        Span::styled(
            format!("{:<width$}", lane.label(), width = label_width),
            label_style,
        ),
        Span::styled(if idle { " idle " } else { "      " }, label_style),
    ];
    let start = viewport_start(selected_slot.unwrap_or(0), lane.items.len(), chip_capacity);
    for (slot, item) in lane
        .items
        .iter()
        .enumerate()
        .skip(start)
        .take(chip_capacity)
    {
        spans.push(runway_chip(item, selected_slot == Some(slot)));
        spans.push(Span::raw(" "));
    }
    let hidden = lane.items.len().saturating_sub(start + chip_capacity);
    if hidden > 0 {
        spans.push(Span::styled(
            format!("+{hidden}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

fn runway_chip(item: &RunwayItem, selected: bool) -> Span<'static> {
    let mut style = match &item.kind {
        RunwayItemKind::Running => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        RunwayItemKind::Ready => Style::default().fg(Color::Cyan),
        RunwayItemKind::Conflicted { .. } => Style::default().fg(Color::Yellow),
    };
    if selected {
        style = style.bg(Color::DarkGray);
    }
    let mut text = match &item.kind {
        RunwayItemKind::Running => format!("\u{25b6}{}", item.id),
        RunwayItemKind::Ready => item.id.clone(),
        RunwayItemKind::Conflicted { holders } => {
            format!("~{} wait:{}", item.id, holders.join(","))
        }
    };
    text.push(' ');
    text.push_str(&item.title);
    if !item.extra_areas.is_empty() {
        text.push_str(" +");
        text.push_str(&item.extra_areas.join("+"));
    }
    Span::styled(
        format!("[{:<width$}]", truncate(&text, CHIP_WIDTH), width = CHIP_WIDTH),
        style,
    )
}

// Build task_id -> sprint_id lookup from sprint index files.
fn task_sprint_map(sprints: &[Sprint]) -> HashMap<&str, &str> {
    sprints
        .iter()
        .flat_map(|s| {
            s.task_ids
                .iter()
                .map(move |e| (numeric_prefix(e), s.header.id.as_str()))
        })
        .collect()
}

fn task_matches_filter(
    task: &Task,
    needle: &str,
    task_sprint: &HashMap<&str, &str>,
    done: &HashSet<&str>,
) -> bool {
    classify(task, done).as_str().contains(needle)
        || task_sprint
            .get(task.frontmatter.id.as_str())
            .unwrap_or(&"")
            .to_lowercase()
            .contains(needle)
        || task
            .frontmatter
            .area
            .iter()
            .any(|area| area.to_lowercase().contains(needle))
        || task
            .frontmatter
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(needle))
}

fn task_matches_text(task: &Task, needle: &str) -> bool {
    task.frontmatter.id.to_lowercase().contains(needle)
        || task.frontmatter.title.to_lowercase().contains(needle)
        || task.body.to_lowercase().contains(needle)
        || task
            .frontmatter
            .area
            .iter()
            .any(|area| area.to_lowercase().contains(needle))
        || task
            .frontmatter
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(needle))
}

fn is_closed_task(task: &Task) -> bool {
    matches!(
        task.frontmatter.status,
        TaskStatus::Done | TaskStatus::Archived
    )
}

fn compare_created_at(a: &Task, b: &Task) -> std::cmp::Ordering {
    match (
        parse_timestamp(a.frontmatter.created_at.as_deref()),
        parse_timestamp(b.frontmatter.created_at.as_deref()),
    ) {
        (Some(a_created), Some(b_created)) => a_created
            .cmp(&b_created)
            .then(a.frontmatter.id.cmp(&b.frontmatter.id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.frontmatter.id.cmp(&b.frontmatter.id),
    }
}

fn parse_timestamp(value: Option<&str>) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    value.and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
}

fn numeric_prefix(entry: &str) -> &str {
    let trimmed = entry.trim();
    let without_link = trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches(".md");
    let end = without_link
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(without_link.len());
    &without_link[..end]
}

fn format_blockers_inline(blockers: &[BlockedByRef]) -> String {
    blockers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

fn selected_summary(task: Option<&Task>) -> String {
    match task {
        Some(task) => format!(
            "selected:{} {}",
            task.frontmatter.id, task.frontmatter.title
        ),
        None => "selected:-".to_owned(),
    }
}

fn table_body_rows(area: Rect) -> usize {
    // Table block borders take two rows and the header takes one.
    usize::from(area.height.saturating_sub(3))
}

fn list_body_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(2))
}

fn viewport_start(selected: usize, total: usize, visible_rows: usize) -> usize {
    if total == 0 || visible_rows == 0 {
        return 0;
    }
    let max_start = total.saturating_sub(visible_rows);
    let selected = selected.min(total - 1);
    selected
        .saturating_add(1)
        .saturating_sub(visible_rows)
        .min(max_start)
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    let mut out = value
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

fn wrap_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    if delta.is_negative() {
        (current + len - ((-delta) as usize % len)) % len
    } else {
        (current + delta as usize) % len
    }
}

fn plain_key(key: KeyEvent) -> bool {
    key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn prompt_rect(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(4),
        y: area.y.saturating_add(area.height.saturating_sub(7) / 2),
        width: area.width.saturating_sub(8),
        height: 7.min(area.height),
    }
}

fn read_optional(path: &PathBuf) -> anyhow::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

enum SnapshotSide {
    Before,
    After,
}

fn apply_snapshots(files: &[FileSnapshot], side: SnapshotSide) -> anyhow::Result<()> {
    for file in files {
        let content = match side {
            SnapshotSide::Before => &file.before,
            SnapshotSide::After => &file.after,
        };
        match content {
            Some(content) => fs::write(&file.path, content)
                .with_context(|| format!("write {}", file.path.display()))?,
            None => {
                if file.path.exists() {
                    fs::remove_file(&file.path)
                        .with_context(|| format!("remove {}", file.path.display()))?;
                }
            }
        }
    }
    Ok(())
}

fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn open_editor_path(path: &PathBuf) -> anyhow::Result<()> {
    if std::env::var_os("STINT_TEST_EDITOR").as_deref() == Some(std::ffi::OsStr::new("none")) {
        return Ok(());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_owned());
    let status = Command::new(editor)
        .arg(path)
        .status()
        .with_context(|| format!("open editor for {}", path.display()))?;
    if !status.success() {
        bail!("editor exited with status {status}");
    }
    Ok(())
}

fn expand_command(template: &str, task: &Task, path: &PathBuf, sprints: &[Sprint]) -> String {
    let fm = &task.frontmatter;
    let sprint_id = sprints
        .iter()
        .find(|s| {
            s.task_ids
                .iter()
                .any(|e| numeric_prefix(e) == fm.id.as_str())
        })
        .map(|s| s.header.id.as_str())
        .unwrap_or("");
    template
        .replace("{id}", &fm.id)
        .replace("{slug}", &title_to_slug(&fm.title))
        .replace("{path}", &shell_quote(&path.to_string_lossy()))
        .replace("{title}", &shell_quote(&fm.title))
        .replace("{sprint}", sprint_id)
        .replace("{area}", &shell_quote(&fm.area.join(",")))
        .replace("{tags}", &shell_quote(&fm.tags.join(",")))
        .replace(
            "{estimate}",
            &fm.estimate
                .map(|estimate| estimate.to_string())
                .unwrap_or_default(),
        )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_shell_command(command: &str) -> anyhow::Result<bool> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .status()
        .with_context(|| format!("run custom command {command:?}"))?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stint::parse::parse_task;

    fn task(id: &str, title: &str, status: &str, extra: &str) -> Task {
        let content =
            format!("---\nid: \"{id}\"\ntitle: \"{title}\"\nstatus: {status}\n{extra}\n---\n");
        parse_task(&content, &format!("{id}-task.md")).unwrap()
    }

    fn model(tasks: &[Task]) -> RunwayModel {
        build_runway_model(tasks, &[], "", "")
    }

    fn lane_names(model: &RunwayModel) -> Vec<&str> {
        model.lanes.iter().map(|lane| lane.label()).collect()
    }

    fn lane_ids(model: &RunwayModel, lane: usize) -> Vec<&str> {
        model.lanes[lane]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect()
    }

    #[test]
    fn lanes_group_by_first_area_alphabetically_with_unassigned_last() {
        let tasks = vec![
            task("0001", "A", "todo", "area: [network]"),
            task("0002", "B", "in-progress", "area: [cli]"),
            task("0003", "C", "todo", ""),
        ];
        let model = model(&tasks);
        assert_eq!(lane_names(&model), vec!["cli", "network", "unassigned"]);
        assert_eq!(lane_ids(&model, 0), vec!["0002"]);
        assert_eq!(lane_ids(&model, 1), vec!["0001"]);
        assert_eq!(lane_ids(&model, 2), vec!["0003"]);
        assert!(model.parked.is_empty());
    }

    #[test]
    fn running_task_precedes_ready_queue_in_its_lane() {
        let tasks = vec![
            task("0001", "A", "todo", "area: [cli]"),
            task("0002", "B", "in-progress", "area: [cli]"),
        ];
        let model = model(&tasks);
        assert_eq!(lane_ids(&model, 0), vec!["0002", "0001"]);
        assert_eq!(model.lanes[0].items[0].kind, RunwayItemKind::Running);
        assert!(matches!(
            model.lanes[0].items[1].kind,
            RunwayItemKind::Conflicted { .. }
        ));
    }

    #[test]
    fn conflicted_item_names_the_holding_task() {
        let tasks = vec![
            task("0001", "A", "in-progress", "area: [cli]"),
            task("0002", "B", "todo", "area: [cli]"),
        ];
        let model = model(&tasks);
        let RunwayItemKind::Conflicted { holders } = &model.lanes[0].items[1].kind else {
            panic!("expected conflicted item");
        };
        assert_eq!(holders, &vec!["0001".to_owned()]);
    }

    #[test]
    fn dependency_blocked_todos_are_parked_with_blockers() {
        let tasks = vec![
            task("0001", "A", "todo", ""),
            task("0002", "B", "todo", "blocked_by: [\"0001\"]"),
        ];
        let model = model(&tasks);
        assert_eq!(model.parked.len(), 1);
        assert_eq!(model.parked[0].id, "0002");
        assert_eq!(model.parked[0].blockers.len(), 1);
    }

    #[test]
    fn multi_area_task_renders_once_with_extra_area_annotation() {
        let tasks = vec![task("0001", "A", "todo", "area: [cli, docs]")];
        let model = model(&tasks);
        assert_eq!(lane_names(&model), vec!["cli"]);
        assert_eq!(model.lanes[0].items[0].extra_areas, vec!["docs".to_owned()]);
    }

    #[test]
    fn ready_queue_keeps_next_order_within_lane() {
        let tasks = vec![
            task("0001", "Low", "todo", "area: [cli]\npriority: p3"),
            task("0002", "High", "todo", "area: [cli]\npriority: p0"),
        ];
        let model = model(&tasks);
        assert_eq!(lane_ids(&model, 0), vec!["0002", "0001"]);
    }

    #[test]
    fn search_drops_items_and_empty_lanes() {
        let tasks = vec![
            task("0001", "Parser work", "todo", "area: [cli]"),
            task("0002", "Docs pass", "todo", "area: [docs]"),
        ];
        let model = build_runway_model(&tasks, &[], "parser", "");
        assert_eq!(lane_names(&model), vec!["cli"]);
    }

    #[test]
    fn backlog_tasks_are_excluded_from_the_runway() {
        let tasks = vec![task("0001", "A", "backlog", "area: [cli]")];
        let model = model(&tasks);
        assert!(model.lanes.is_empty());
        assert!(model.parked.is_empty());
    }

    #[test]
    fn flat_traversal_roundtrips_lane_and_slot() {
        let tasks = vec![
            task("0001", "A", "todo", "area: [cli]"),
            task("0002", "B", "in-progress", "area: [cli]"),
            task("0003", "C", "todo", "area: [docs]"),
            task("0004", "D", "todo", "blocked_by: [\"0003\"]"),
        ];
        let model = model(&tasks);
        assert_eq!(model.flat_ids(), vec!["0002", "0001", "0003", "0004"]);
        assert_eq!(model.locate(1), Some((0, 1)));
        assert_eq!(model.locate(2), Some((1, 0)));
        assert_eq!(model.locate(3), Some((2, 0)));
        assert_eq!(model.flat_of(2, 0), 3);
        assert_eq!(model.locate(4), None);
    }

    #[test]
    fn viewport_start_keeps_selected_row_visible() {
        assert_eq!(viewport_start(0, 20, 5), 0);
        assert_eq!(viewport_start(4, 20, 5), 0);
        assert_eq!(viewport_start(5, 20, 5), 1);
        assert_eq!(viewport_start(19, 20, 5), 15);
    }

    #[test]
    fn viewport_start_handles_empty_and_oversized_inputs() {
        assert_eq!(viewport_start(0, 0, 5), 0);
        assert_eq!(viewport_start(0, 20, 0), 0);
        assert_eq!(viewport_start(99, 3, 10), 0);
    }
}
