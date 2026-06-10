mod cmds;
mod repo;

use std::env;
use std::process;

use anyhow::Context;
use clap::{Parser, Subcommand};

use repo::StintRepo;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "stint",
    about = "Git-tracked sprint planning and task tracking"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new task and open $EDITOR for the body.
    Add {
        /// Task title.
        title: String,
    },

    /// List tasks, optionally filtered.
    List {
        /// Filter by status (backlog|todo|in-progress|done|archived).
        #[arg(long)]
        status: Option<String>,
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

    /// Mark a task done and optionally record actual time.
    Done {
        /// Task ID.
        id: String,
        /// Actual time spent (e.g. 2h, 30m). Skips the prompt when provided.
        #[arg(long)]
        actual: Option<String>,
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
        /// Mark the top ready task as in-progress.
        #[arg(long)]
        claim: bool,
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
        Commands::Add { title } => {
            let repo = find_repo()?;
            let path = cmds::cmd_add(&repo, &title)?;
            println!("{}", path.display());
        }

        Commands::List {
            status,
            sprint,
            area,
            tag,
        } => {
            let repo = find_repo()?;
            let rows = cmds::cmd_list(
                &repo,
                status.as_deref(),
                sprint.as_deref(),
                area.as_deref(),
                tag.as_deref(),
            )?;
            cmds::print_list(&rows);
        }

        Commands::Show { id } => {
            let repo = find_repo()?;
            cmds::cmd_show(&repo, &id)?;
        }

        Commands::Edit { id } => {
            let repo = find_repo()?;
            cmds::cmd_edit(&repo, &id)?;
        }

        Commands::Done { id, actual } => {
            let repo = find_repo()?;
            let path = cmds::cmd_done(&repo, &id, actual.as_deref())?;
            println!("done: {}", path.display());
        }

        Commands::Log { id, duration } => {
            let repo = find_repo()?;
            let path = cmds::cmd_log(&repo, &id, &duration)?;
            println!("logged: {}", path.display());
        }

        Commands::Archive { id } => {
            let repo = find_repo()?;
            let path = cmds::cmd_archive(&repo, &id)?;
            println!("archived: {}", path.display());
        }

        Commands::Next {
            sprint,
            include_area_conflicts,
            claim,
        } => {
            let repo = find_repo()?;
            let report = cmds::cmd_next(&repo, sprint.as_deref(), include_area_conflicts, claim)?;
            cmds::print_next(&report, claim);
        }

        Commands::Sprint { command } => match command {
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

        Commands::Check { cross_repo } => {
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

        Commands::Status => {
            let repo = find_repo()?;
            cmds::cmd_status(&repo)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn find_repo() -> anyhow::Result<StintRepo> {
    let cwd = env::current_dir().context("get current directory")?;
    StintRepo::find(&cwd)
}
