mod cmds;

use std::env;
use std::io::IsTerminal;
use std::process::{self, Command};

use anyhow::Context;
use clap::{Parser, Subcommand};

use stint::repo::StintRepo;
use stint::tui;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "stint",
    version,
    about = "Git-tracked sprint planning and task tracking"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a .stint workspace in the current directory.
    Init {
        /// Ensure missing files when .stint already exists.
        #[arg(long)]
        force: bool,
        /// Import open GitHub issues through the gh CLI.
        #[arg(long)]
        with_github: bool,
        /// GitHub repository override for --with-github, e.g. owner/name.
        #[arg(long)]
        repo: Option<String>,
    },

    /// Create a new task and open $EDITOR for the body.
    Add {
        /// Task title.
        title: String,
    },

    /// List tasks, optionally filtered.
    #[command(alias = "ls")]
    List {
        /// Filter by status (backlog|todo|in-progress|done|archived).
        #[arg(long)]
        status: Option<String>,
        /// Include done and archived tasks when no explicit status filter is set.
        #[arg(long)]
        all: bool,
        /// Show only tasks that are currently blocked.
        #[arg(long)]
        blocked: bool,
        /// Hide tasks that are currently blocked.
        #[arg(long)]
        hide_blocked: bool,
        /// Filter by sprint ID (e.g. s12).
        #[arg(long)]
        sprint: Option<String>,
        /// Filter by area label.
        #[arg(long)]
        area: Option<String>,
        /// Filter by tag.
        #[arg(long)]
        tag: Option<String>,
    },

    /// Print a full task (frontmatter + body).
    Show {
        /// Task ID (full, partial, or with slug).
        id: String,
    },

    /// Open a task file in $EDITOR.
    Edit {
        /// Task ID.
        id: String,
    },

    /// Remove one or more task files.
    #[command(alias = "rm")]
    Remove {
        /// Task IDs.
        #[arg(required = true)]
        ids: Vec<String>,
    },

    /// Mark a task in-progress and record started_at.
    Start {
        /// Task ID.
        id: String,
        /// Replace an existing started_at and clear completion fields.
        #[arg(long)]
        restart: bool,
        /// Start timestamp, UTC RFC3339 preferred. Defaults to now.
        #[arg(long)]
        started_at: Option<String>,
    },

    /// Mark a task done and optionally record actual time.
    Done {
        /// Task ID.
        id: String,
        /// Actual time spent (e.g. 2h, 30m). Skips the prompt when provided.
        #[arg(long)]
        actual: Option<String>,
        /// Start timestamp to use if started_at is missing.
        #[arg(long)]
        started_at: Option<String>,
        /// Completion timestamp. Defaults to now.
        #[arg(long)]
        completed_at: Option<String>,
    },

    /// Add time to a task's actual field (accumulates).
    Log {
        /// Task ID.
        id: String,
        /// Duration to add (e.g. 2h, 30m).
        duration: String,
    },

    /// Set a task's status to archived.
    Archive {
        /// Task ID.
        id: String,
    },

    /// Show the next claimable tasks, derived from blockers, sprint order, and area conflicts.
    Next {
        /// Restrict to a sprint ID (e.g. s12).
        #[arg(long)]
        sprint: Option<String>,
        /// Include tasks whose area conflicts with in-progress work.
        #[arg(long)]
        include_area_conflicts: bool,
        /// Include backlog tasks alongside todo tasks (opt-in; default excludes backlog).
        #[arg(long)]
        include_backlog: bool,
        /// Mark the top ready task(s) as in-progress (atomic with count).
        #[arg(long)]
        claim: bool,
        /// Maximum number of ready tasks to return (default: all).
        #[arg(long)]
        count: Option<usize>,
        /// Output as JSON for agent/orchestrator consumption.
        #[arg(long)]
        json: bool,
    },

    /// Promote backlog tasks to todo (move out of icebox into the ready pool).
    Ready {
        /// Task ID(s) to promote. Omit to use --sprint or --tag selectors.
        ids: Vec<String>,
        /// Promote all backlog tasks in this sprint.
        #[arg(long)]
        sprint: Option<String>,
        /// Promote all backlog tasks with this tag.
        #[arg(long)]
        tag: Option<String>,
    },

    /// Send todo tasks back to backlog (move into icebox).
    Defer {
        /// Task ID(s) to defer. Omit to use --sprint or --tag selectors.
        ids: Vec<String>,
        /// Defer all todo tasks in this sprint.
        #[arg(long)]
        sprint: Option<String>,
        /// Defer all todo tasks with this tag.
        #[arg(long)]
        tag: Option<String>,
    },

    /// Sprint management sub-commands.
    Sprint {
        #[command(subcommand)]
        command: SprintCommands,
    },

    /// Validate the entire task graph; exit 1 if any errors are found.
    Check {
        /// Walk sibling repositories and validate cross-repo references (stub; not yet implemented).
        #[arg(long)]
        cross_repo: bool,
    },

    /// Show a summary: open tasks, blocked tasks, current sprint progress.
    Status,

    /// Update stint from crates.io through cargo install.
    Update,
}

#[derive(Subcommand)]
enum SprintCommands {
    /// Create a new sprint file.
    New {
        /// Sprint ID (e.g. s12 or 12).
        id: String,
        /// Date range string (e.g. "Jun 9-20").
        date_range: String,
        /// Sprint goal text.
        #[arg(long)]
        goal: Option<String>,
    },

    /// List all sprints.
    List,

    /// Show a sprint with task summaries.
    Show {
        /// Sprint ID.
        id: String,
    },

    /// Append a task to a sprint file.
    Add {
        /// Sprint ID.
        sprint_id: String,
        /// Task ID.
        task_id: String,
    },

    /// Remove a task from a sprint file.
    Remove {
        /// Sprint ID.
        sprint_id: String,
        /// Task ID.
        task_id: String,
    },

    /// Open a sprint file in $EDITOR for manual reordering.
    Reorder {
        /// Sprint ID.
        id: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {:#}", e);
        process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        None => {
            let repo = find_repo()?;
            if std::io::stdout().is_terminal() {
                tui::run(repo)?;
            } else {
                cmds::cmd_status(&repo)?;
            }
        }
        Some(Commands::Init {
            force,
            with_github,
            repo,
        }) => {
            let cwd = env::current_dir().context("get current directory")?;
            let report = cmds::cmd_init(&cwd, force, with_github, repo.as_deref())?;
            println!("initialized: {}", report.repo.display());
            if with_github {
                println!(
                    "github import: {} imported, {} skipped",
                    report.imported, report.skipped
                );
            }
        }

        Some(Commands::Add { title }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_add(&repo, &title)?;
            println!("{}", path.display());
        }

        Some(Commands::List {
            status,
            all,
            blocked,
            hide_blocked,
            sprint,
            area,
            tag,
        }) => {
            let repo = find_repo()?;
            if blocked && hide_blocked {
                anyhow::bail!("--blocked and --hide-blocked cannot be used together");
            }
            let blocked_filter = if blocked {
                Some(true)
            } else if hide_blocked {
                Some(false)
            } else {
                None
            };
            let rows = cmds::cmd_list(
                &repo,
                status.as_deref(),
                all,
                blocked_filter,
                sprint.as_deref(),
                area.as_deref(),
                tag.as_deref(),
            )?;
            cmds::print_list(&rows);
        }

        Some(Commands::Show { id }) => {
            let repo = find_repo()?;
            cmds::cmd_show(&repo, &id)?;
        }

        Some(Commands::Edit { id }) => {
            let repo = find_repo()?;
            cmds::cmd_edit(&repo, &id)?;
        }

        Some(Commands::Remove { ids }) => {
            let repo = find_repo()?;
            let paths = cmds::cmd_remove(&repo, &ids)?;
            for path in paths {
                println!("removed: {}", path.display());
            }
        }

        Some(Commands::Start {
            id,
            restart,
            started_at,
        }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_start(&repo, &id, restart, started_at.as_deref())?;
            println!("started: {}", path.display());
        }

        Some(Commands::Done {
            id,
            actual,
            started_at,
            completed_at,
        }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_done(
                &repo,
                &id,
                actual.as_deref(),
                started_at.as_deref(),
                completed_at.as_deref(),
            )?;
            println!("done: {}", path.display());
            let unblocked = cmds::tasks_unblocked_by_done(&repo, &id)?;
            if !unblocked.is_empty() {
                println!("unblocked:");
                for task in unblocked {
                    println!("  {} {}", task.id, task.title);
                }
            }
        }

        Some(Commands::Log { id, duration }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_log(&repo, &id, &duration)?;
            println!("logged: {}", path.display());
        }

        Some(Commands::Archive { id }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_archive(&repo, &id)?;
            println!("archived: {}", path.display());
        }

        Some(Commands::Next {
            sprint,
            include_area_conflicts,
            include_backlog,
            claim,
            count,
            json,
        }) => {
            let repo = find_repo()?;
            let report = cmds::cmd_next(
                &repo,
                sprint.as_deref(),
                include_area_conflicts,
                include_backlog,
                claim,
                count,
            )?;
            if json {
                cmds::print_next_json(&repo, &report, claim, count);
            } else {
                cmds::print_next(&report, claim, count);
            }
        }

        Some(Commands::Ready { ids, sprint, tag }) => {
            let repo = find_repo()?;
            let paths = cmds::cmd_ready(&repo, &ids, sprint.as_deref(), tag.as_deref())?;
            for path in paths {
                println!("ready: {}", path.display());
            }
        }

        Some(Commands::Defer { ids, sprint, tag }) => {
            let repo = find_repo()?;
            let paths = cmds::cmd_defer(&repo, &ids, sprint.as_deref(), tag.as_deref())?;
            for path in paths {
                println!("deferred: {}", path.display());
            }
        }

        Some(Commands::Sprint { command }) => match command {
            SprintCommands::New {
                id,
                date_range,
                goal,
            } => {
                let repo = find_repo()?;
                let path = cmds::cmd_sprint_new(&repo, &id, &date_range, goal.as_deref())?;
                println!("{}", path.display());
            }
            SprintCommands::List => {
                let repo = find_repo()?;
                cmds::cmd_sprint_list(&repo)?;
            }
            SprintCommands::Show { id } => {
                let repo = find_repo()?;
                cmds::cmd_sprint_show(&repo, &id)?;
            }
            SprintCommands::Add { sprint_id, task_id } => {
                let repo = find_repo()?;
                let path = cmds::cmd_sprint_add(&repo, &sprint_id, &task_id)?;
                println!("added to {}", path.display());
            }
            SprintCommands::Remove { sprint_id, task_id } => {
                let repo = find_repo()?;
                let path = cmds::cmd_sprint_remove(&repo, &sprint_id, &task_id)?;
                println!("removed from {}", path.display());
            }
            SprintCommands::Reorder { id } => {
                let repo = find_repo()?;
                let path = cmds::cmd_sprint_reorder(&repo, &id)?;
                println!("reordered {}", path.display());
            }
        },

        Some(Commands::Check { cross_repo }) => {
            let repo = find_repo()?;
            let errors = cmds::cmd_check(&repo, cross_repo)?;
            if errors.is_empty() {
                println!("ok");
            } else {
                for e in &errors {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }

        Some(Commands::Status) => {
            let repo = find_repo()?;
            cmds::cmd_status(&repo)?;
        }

        Some(Commands::Update) => {
            cmd_update()?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn cmd_update() -> anyhow::Result<()> {
    let status = Command::new("cargo")
        .args(["install", "stint", "--locked", "--force"])
        .status()
        .context("run cargo install stint --locked --force")?;

    if !status.success() {
        anyhow::bail!("cargo install stint --locked --force failed");
    }

    Ok(())
}

fn find_repo() -> anyhow::Result<StintRepo> {
    let cwd = env::current_dir().context("get current directory")?;
    StintRepo::find(&cwd)
}
