use std::collections::HashMap;
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
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
};
use ratatui::{Frame, Terminal};
use serde::Deserialize;
use stint::check::check;
use stint::mutate::{
    new_task_content, next_task_id, set_completed_at, set_started_at_if_absent, set_status,
    title_to_slug,
};
use stint::next::{compute_next, NextOptions, NextReport};
use stint::schema::{BlockedByRef, Sprint, Task, TaskStatus};
use stint::serialize::serialize_task;
use stint::state::{active_blockers, classify, done_ids, TaskState};
use stint::status::{compute_status, StatusReport};

use crate::repo::StintRepo;

type Term = Terminal<CrosstermBackend<Stdout>>;

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
    Dashboard,
    Board,
    Table,
    Sprint,
}

impl View {
    fn all() -> [Self; 4] {
        [Self::Dashboard, Self::Board, Self::Table, Self::Sprint]
    }

    fn label(self) -> &'static str {
        match self {
            View::Dashboard => "Dashboard",
            View::Board => "Board",
            View::Table => "Table",
            View::Sprint => "Sprint",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Id,
    State,
    Sprint,
}

impl SortMode {
    fn next(self) -> Self {
        match self {
            SortMode::Id => SortMode::State,
            SortMode::State => SortMode::Sprint,
            SortMode::Sprint => SortMode::Id,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortMode::Id => "id",
            SortMode::State => "state",
            SortMode::Sprint => "sprint",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    Search,
    Filter,
    NewTask { edit_after: bool },
    Command,
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
    #[serde(default)]
    command: Vec<CommandConfig>,
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
    next: NextReport,
    status: StatusReport,
    validation_errors: Vec<String>,
}

struct App {
    repo: StintRepo,
    data: AppData,
    view: View,
    selected: usize,
    board_column: usize,
    sprint_index: usize,
    sort: SortMode,
    search: String,
    filter: String,
    prompt: Option<PromptKind>,
    input: String,
    show_detail: bool,
    command_index: usize,
    custom_menu: bool,
    custom_index: usize,
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
        let custom_commands = load_custom_commands(&repo);
        Ok(Self {
            repo,
            data,
            view: View::Dashboard,
            selected: 0,
            board_column: 0,
            sprint_index: 0,
            sort: SortMode::Id,
            search: String::new(),
            filter: String::new(),
            prompt: None,
            input: String::new(),
            show_detail: false,
            command_index: 0,
            custom_menu: false,
            custom_index: 0,
            custom_commands,
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
                self.custom_commands = load_custom_commands(&self.repo);
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
        let done = done_ids(&self.data.tasks);
        let mut tasks: Vec<&Task> = match self.view {
            View::Dashboard => self
                .data
                .tasks
                .iter()
                .filter(|task| matches!(task.frontmatter.status, TaskStatus::InProgress))
                .collect(),
            View::Board => {
                let state = board_states()[self.board_column];
                self.data
                    .tasks
                    .iter()
                    .filter(|task| classify(task, &done) == state)
                    .collect()
            }
            View::Sprint => self.sprint_tasks(),
            View::Table => self.data.tasks.iter().collect(),
        };

        if !self.search.is_empty() {
            let needle = self.search.to_lowercase();
            tasks.retain(|task| task_matches_text(task, &needle));
        }
        if !self.filter.is_empty() {
            let needle = self.filter.to_lowercase();
            tasks.retain(|task| {
                classify(task, &done).as_str().contains(&needle)
                    || task
                        .frontmatter
                        .sprint
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&needle)
                    || task
                        .frontmatter
                        .area
                        .iter()
                        .any(|area| area.to_lowercase().contains(&needle))
                    || task
                        .frontmatter
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&needle))
            });
        }

        match self.sort {
            SortMode::Id => tasks.sort_by(|a, b| a.frontmatter.id.cmp(&b.frontmatter.id)),
            SortMode::State => tasks.sort_by(|a, b| {
                classify(a, &done)
                    .as_str()
                    .cmp(classify(b, &done).as_str())
                    .then(a.frontmatter.id.cmp(&b.frontmatter.id))
            }),
            SortMode::Sprint => tasks.sort_by(|a, b| {
                a.frontmatter
                    .sprint
                    .cmp(&b.frontmatter.sprint)
                    .then(a.frontmatter.id.cmp(&b.frontmatter.id))
            }),
        }
        tasks
    }

    fn sprint_tasks(&self) -> Vec<&Task> {
        let Some(sprint) = self.data.sprints.get(self.sprint_index) else {
            return Vec::new();
        };
        let by_id: HashMap<&str, &Task> = self
            .data
            .tasks
            .iter()
            .map(|task| (task.frontmatter.id.as_str(), task))
            .collect();
        sprint
            .task_ids
            .iter()
            .filter_map(|entry| by_id.get(numeric_prefix(entry)).copied())
            .collect()
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
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(4),
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        match self.view {
            View::Dashboard => self.render_dashboard(frame, chunks[1]),
            View::Board => self.render_board(frame, chunks[1]),
            View::Table => self.render_table(frame, chunks[1]),
            View::Sprint => self.render_sprint(frame, chunks[1]),
        }
        self.render_footer(frame, chunks[2]);

        if self.show_detail {
            self.render_detail(frame, centered_rect(78, 82, area));
        }
        if self.prompt.is_some() {
            self.render_prompt(frame, prompt_rect(area));
        }
        if self.custom_menu {
            self.render_custom_menu(frame, centered_rect(64, 60, area));
        }
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
            .block(Block::default().borders(Borders::ALL).title("stint"))
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, area);
    }

    fn render_dashboard(&self, frame: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
            .split(area);

        let tasks = self.visible_tasks();
        frame.render_widget(
            task_list("Active", &tasks, self.selected, &self.data.tasks),
            chunks[0],
        );

        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Open ", Style::default().fg(Color::Gray)),
            Span::raw(self.data.status.open_count.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Backlog ", Style::default().fg(Color::Gray)),
            Span::raw(self.data.status.backlog_count.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Blocked ", Style::default().fg(Color::Gray)),
            Span::raw(self.data.status.blocked_tasks.len().to_string()),
        ]));
        lines.push(Line::from(""));
        if let Some(progress) = &self.data.status.sprint_progress {
            lines.push(Line::styled(
                format!("{} progress", progress.sprint_id),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(format!(
                "{} / {} tasks done",
                progress.done_count, progress.task_count
            )));
            lines.push(Line::from(format!(
                "{} logged - {} committed - {} remaining",
                fmt_minutes(progress.logged_minutes),
                fmt_minutes(progress.committed_minutes),
                fmt_minutes(progress.remaining_minutes)
            )));
        }
        lines.push(Line::from(""));
        if let Some(bottleneck) = &self.data.next.bottleneck {
            lines.push(Line::styled(
                "Bottleneck",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(format!(
                "{} {} (blocks {})",
                bottleneck.id, bottleneck.title, bottleneck.blocked_count
            )));
        }
        if !self.data.validation_errors.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::styled(
                format!("{} validation error(s)", self.data.validation_errors.len()),
                Style::default().fg(Color::Red),
            ));
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("Summary"))
                .wrap(Wrap { trim: true }),
            chunks[1],
        );
    }

    fn render_board(&self, frame: &mut Frame<'_>, area: Rect) {
        let states = board_states();
        let constraints = states.map(|_| Constraint::Percentage(20));
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);
        let done = done_ids(&self.data.tasks);
        for (index, state) in states.iter().enumerate() {
            let tasks = self
                .data
                .tasks
                .iter()
                .filter(|task| classify(task, &done) == *state)
                .collect::<Vec<_>>();
            let selected = if index == self.board_column {
                self.selected
            } else {
                usize::MAX
            };
            let title = format!("{} {}", state.as_str(), tasks.len());
            frame.render_widget(
                task_list(&title, &tasks, selected, &self.data.tasks),
                columns[index],
            );
        }
    }

    fn render_table(&self, frame: &mut Frame<'_>, area: Rect) {
        let done = done_ids(&self.data.tasks);
        let rows = self
            .visible_tasks()
            .iter()
            .enumerate()
            .map(|(index, task)| {
                let style = if index == self.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(task.frontmatter.id.clone()),
                    Cell::from(classify(task, &done).as_str()),
                    Cell::from(
                        task.frontmatter
                            .estimate
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_owned()),
                    ),
                    Cell::from(
                        task.frontmatter
                            .sprint
                            .clone()
                            .unwrap_or_else(|| "-".to_owned()),
                    ),
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
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(16),
                Constraint::Min(16),
            ],
        )
        .header(
            Row::new(["ID", "STATE", "EST", "SPRINT", "AREA", "TITLE"])
                .style(Style::default().fg(Color::Gray)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Table - sort {}", self.sort.label())),
        );
        frame.render_widget(table, area);
    }

    fn render_sprint(&self, frame: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(5)])
            .split(area);

        if let Some(sprint) = self.data.sprints.get(self.sprint_index) {
            let progress = sprint_progress_for(&self.data.tasks, &sprint.header.id);
            let ratio = if progress.task_count == 0 {
                0.0
            } else {
                progress.done_count as f64 / progress.task_count as f64
            };
            let label = format!(
                "{} - {} - {} / {} done - {} logged / {} committed",
                sprint.header.id,
                sprint.header.date_range,
                progress.done_count,
                progress.task_count,
                fmt_minutes(progress.logged_minutes),
                fmt_minutes(progress.committed_minutes)
            );
            frame.render_widget(
                Gauge::default()
                    .block(
                        Block::default().borders(Borders::ALL).title(
                            sprint
                                .header
                                .goal
                                .clone()
                                .unwrap_or_else(|| "Sprint".to_owned()),
                        ),
                    )
                    .gauge_style(Style::default().fg(Color::Cyan))
                    .ratio(ratio)
                    .label(label),
                chunks[0],
            );
        } else {
            frame.render_widget(
                Paragraph::new("no sprints")
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::ALL).title("Sprint")),
                chunks[0],
            );
        }

        let tasks = self.visible_tasks();
        frame.render_widget(
            task_list("Sprint tasks", &tasks, self.selected, &self.data.tasks),
            chunks[1],
        );
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let lines = if self.message.is_empty() || self.message_at.elapsed().as_secs() > 8 {
            vec![
                Line::styled(
                    "tab view - shift-tab back - hjkl/arrows move - enter detail - e edit",
                    Style::default().fg(Color::Gray),
                ),
                Line::styled(
                    "c claim - d done - r ready - b defer - a archive - n new - N new+edit",
                    Style::default().fg(Color::Gray),
                ),
                Line::from(vec![
                    Span::styled(
                        "/ search - f filter - s sort - x commands - : palette - u undo - ctrl-r redo - q quit",
                        Style::default().fg(Color::Gray),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!(
                            "search:{} filter:{} undo:{} redo:{}",
                            empty_dash(&self.search),
                            empty_dash(&self.filter),
                            self.undo.len(),
                            self.redo.len()
                        ),
                        Style::default().fg(Color::DarkGray),
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
        let mut lines = vec![
            Line::styled(
                format!("{} {}", fm.id, fm.title),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(format!("status: {}", fm.status)),
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
            Line::from(format!("sprint: {}", fm.sprint.as_deref().unwrap_or("-"))),
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

    fn render_prompt(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        let title = match self.prompt {
            Some(PromptKind::Search) => "Search",
            Some(PromptKind::Filter) => "Filter",
            Some(PromptKind::NewTask { edit_after: false }) => "New task",
            Some(PromptKind::NewTask { edit_after: true }) => "New task + edit",
            Some(PromptKind::Command) => "Command palette",
            None => "",
        };
        let body = if self.prompt == Some(PromptKind::Command) {
            let items = CommandPaletteItem::all();
            items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.label().contains(&self.input))
                .map(|(index, item)| {
                    if index == self.command_index {
                        format!("> {}", item.label())
                    } else {
                        format!("  {}", item.label())
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            self.input.clone()
        };
        frame.render_widget(
            Paragraph::new(body)
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
        if self.prompt.is_some() {
            return self.handle_prompt_key(key, terminal);
        }
        if self.show_detail && key.code == KeyCode::Esc {
            self.show_detail = false;
            return Ok(false);
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
            KeyCode::Char('c') if plain_key(key) => {
                return self.transition_selected("claim", TaskStatus::InProgress, true)
            }
            KeyCode::Char('d') if plain_key(key) => return self.transition_done(),
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return self.redo()
            }
            KeyCode::Char('r') if plain_key(key) => {
                return self.transition_selected("ready", TaskStatus::Todo, false)
            }
            KeyCode::Char('b') if plain_key(key) => {
                return self.transition_selected("defer", TaskStatus::Backlog, false)
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
                self.sort = self.sort.next();
                self.set_message(format!("sort: {}", self.sort.label()));
            }
            KeyCode::Char('x') if plain_key(key) => self.custom_menu = true,
            KeyCode::Char(':') if plain_key(key) => self.open_prompt(PromptKind::Command, ""),
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
        let Some(prompt) = self.prompt else {
            return Ok(false);
        };
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.input.clear();
            }
            KeyCode::Enter => {
                let value = self.input.trim().to_owned();
                self.prompt = None;
                self.input.clear();
                match prompt {
                    PromptKind::Search => {
                        self.search = value;
                        self.selected = 0;
                    }
                    PromptKind::Filter => {
                        self.filter = value;
                        self.selected = 0;
                    }
                    PromptKind::NewTask { edit_after } => {
                        if !value.is_empty() {
                            return self.create_task(&value, edit_after, terminal);
                        }
                    }
                    PromptKind::Command => return self.run_palette_item(terminal),
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Down if prompt == PromptKind::Command => {
                self.command_index = (self.command_index + 1) % CommandPaletteItem::all().len();
            }
            KeyCode::Char('j') if prompt == PromptKind::Command && plain_key(key) => {
                self.command_index = (self.command_index + 1) % CommandPaletteItem::all().len();
            }
            KeyCode::Up if prompt == PromptKind::Command => {
                let len = CommandPaletteItem::all().len();
                self.command_index = (self.command_index + len - 1) % len;
            }
            KeyCode::Char('k') if prompt == PromptKind::Command && plain_key(key) => {
                let len = CommandPaletteItem::all().len();
                self.command_index = (self.command_index + len - 1) % len;
            }
            KeyCode::Char(ch) if plain_key(key) => self.input.push(ch),
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
        match CommandPaletteItem::all()[self.command_index] {
            CommandPaletteItem::Claim => {
                self.transition_selected("claim", TaskStatus::InProgress, true)
            }
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
        let len = self.visible_tasks().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = wrap_index(self.selected, len, delta);
    }

    fn move_horizontal(&mut self, delta: isize) {
        match self.view {
            View::Board => {
                self.board_column = wrap_index(self.board_column, board_states().len(), delta);
                self.selected = 0;
            }
            View::Sprint => {
                if !self.data.sprints.is_empty() {
                    self.sprint_index =
                        wrap_index(self.sprint_index, self.data.sprints.len(), delta);
                    self.selected = 0;
                }
            }
            _ => self.move_selection(delta),
        }
    }

    fn open_prompt(&mut self, prompt: PromptKind, value: &str) {
        self.prompt = Some(prompt);
        self.input = value.to_owned();
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
        let path = self.repo.resolve_task_path(&id)?;
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
        let content = new_task_content(&id, title);
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
        let run = expand_command(&command.run, &task, &path);
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
    let next = compute_next(
        &tasks,
        &sprints,
        NextOptions {
            sprint: None,
            include_area_conflicts: false,
            include_backlog: false,
        },
    );
    let status = compute_status(&tasks, &sprints, None);
    let validation_errors = check(&tasks, &sprints)
        .into_iter()
        .map(|error| error.to_string())
        .collect();
    Ok(AppData {
        tasks,
        sprints,
        next,
        status,
        validation_errors,
    })
}

fn load_custom_commands(repo: &StintRepo) -> Vec<CustomCommand> {
    let path = repo.stint_dir.join("config.toml");
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(config) = toml::from_str::<ConfigFile>(&content) else {
        return Vec::new();
    };
    config
        .command
        .into_iter()
        .map(|command| CustomCommand {
            key: command.key.and_then(|key| key.chars().next()),
            name: command.name,
            run: command.run,
            claim: command.claim,
        })
        .collect()
}

fn task_list<'a>(title: &str, tasks: &[&'a Task], selected: usize, all_tasks: &[Task]) -> List<'a> {
    let done = done_ids(all_tasks);
    let items = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let state = classify(task, &done);
            let marker = if index == selected { "> " } else { "  " };
            let style = if index == selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let reason = if state == TaskState::Blocked {
                let blockers = active_blockers(task, &done);
                format!(" - {}", format_blockers_inline(&blockers))
            } else {
                String::new()
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
                Span::styled(state.as_str(), state_style(state)),
                Span::raw(" "),
                Span::raw(truncate(&task.frontmatter.title, 48)),
                Span::styled(reason, Style::default().fg(Color::Yellow)),
            ]))
            .style(style)
        })
        .collect::<Vec<_>>();
    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title.to_owned()),
    )
}

fn state_style(state: TaskState) -> Style {
    match state {
        TaskState::Iced => Style::default().fg(Color::DarkGray),
        TaskState::Ready => Style::default().fg(Color::Green),
        TaskState::Blocked => Style::default().fg(Color::Yellow),
        TaskState::Active => Style::default().fg(Color::Cyan),
        TaskState::Done | TaskState::Archived => Style::default().fg(Color::Gray),
    }
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

fn board_states() -> [TaskState; 5] {
    [
        TaskState::Iced,
        TaskState::Ready,
        TaskState::Blocked,
        TaskState::Active,
        TaskState::Done,
    ]
}

fn sprint_progress_for(tasks: &[Task], sprint_id: &str) -> stint::status::SprintProgress {
    compute_status(tasks, &[], Some(sprint_id))
        .sprint_progress
        .unwrap_or_else(|| {
            let sprint_tasks = tasks
                .iter()
                .filter(|task| task.frontmatter.sprint.as_deref() == Some(sprint_id))
                .collect::<Vec<_>>();
            let committed_minutes = sprint_tasks
                .iter()
                .filter_map(|task| task.frontmatter.estimate)
                .map(|duration| duration.minutes())
                .sum();
            let logged_minutes = sprint_tasks
                .iter()
                .filter_map(|task| task.frontmatter.actual)
                .map(|duration| duration.minutes())
                .sum();
            let done_count = sprint_tasks
                .iter()
                .filter(|task| matches!(task.frontmatter.status, TaskStatus::Done))
                .count();
            stint::status::SprintProgress {
                sprint_id: sprint_id.to_owned(),
                committed_minutes,
                logged_minutes,
                remaining_minutes: committed_minutes.saturating_sub(logged_minutes),
                task_count: sprint_tasks.len(),
                done_count,
            }
        })
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

fn fmt_minutes(minutes: u32) -> String {
    if minutes == 0 {
        "0m".to_owned()
    } else if minutes % 60 == 0 {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
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

fn expand_command(template: &str, task: &Task, path: &PathBuf) -> String {
    let fm = &task.frontmatter;
    template
        .replace("{id}", &fm.id)
        .replace("{slug}", &title_to_slug(&fm.title))
        .replace("{path}", &shell_quote(&path.to_string_lossy()))
        .replace("{title}", &shell_quote(&fm.title))
        .replace("{sprint}", fm.sprint.as_deref().unwrap_or(""))
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
