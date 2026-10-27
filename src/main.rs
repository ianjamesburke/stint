mod cmds;

use std::env;
use std::process::{self, Command};

use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use serde_json::json;

use stint::guide::{format_guide, format_schema, guide_json, schema_json};
use stint::repo::StintRepo;
use stint::schema::TaskStatus;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "stint",
    version,
    about = "Git-tracked sprint planning, agent task management, and decision gating"
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

    /// Create a new task and (when interactive and no body flags given) open $EDITOR.
    Add {
        /// Task title.
        title: String,
        /// Priority level (p0|p1|p2|p3|p4).
        #[arg(long)]
        priority: Option<String>,
        /// Size estimate (s|m|l).
        #[arg(long)]
        size: Option<String>,
        /// Area/component labels (comma-separated or repeated).
        #[arg(long, value_delimiter = ',')]
        area: Vec<String>,
        /// Tags (comma-separated or repeated).
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Blocker references (comma-separated or repeated): local task id,
        /// "@N" for a local GitHub issue, "owner/repo:0001" for an external
        /// task, or a free-text note.
        #[arg(long = "blocked-by", value_delimiter = ',')]
        blocked_by: Vec<String>,
        /// Linked GitHub issue numbers (comma-separated or repeated).
        #[arg(long = "gh-issue", value_delimiter = ',')]
        gh_issue: Vec<String>,
        /// Inline `## Why` problem statement (skips $EDITOR).
        #[arg(long)]
        why: Option<String>,
        /// Inline `## Done When` pass criteria bullet(s) (repeatable; skips $EDITOR).
        #[arg(long = "done-when")]
        done_when: Vec<String>,
        /// Inline `## References` bullet(s) (repeatable; skips $EDITOR).
        #[arg(long)]
        references: Vec<String>,
        /// Body markdown source: a file path, or "-" to read from stdin.
        /// Replaces the default Why/Done When/References template wholesale.
        #[arg(long = "body-file")]
        body_file: Option<String>,
        /// Skip opening $EDITOR after creation.
        #[arg(long)]
        no_edit: bool,
        /// File the task into the backlog (icebox) instead of the todo pool
        /// that `stint next` schedules from.
        #[arg(long)]
        backlog: bool,
        /// File the task in the `proposed` state (outside the active pipeline until `stint approve`).
        #[arg(long)]
        proposed: bool,
        /// Output as JSON for agent/orchestrator consumption.
        #[arg(long)]
        json: bool,
    },

    /// Propose a candidate stint (`status: proposed`) outside the active ready queue.
    Propose {
        /// Proposal title.
        title: String,
        /// Optional priority level (p0|p1|p2|p3|p4).
        #[arg(long)]
        priority: Option<String>,
        /// Optional size estimate (s|m|l).
        #[arg(long)]
        size: Option<String>,
        /// Area/component labels (comma-separated or repeated).
        #[arg(long, value_delimiter = ',')]
        area: Vec<String>,
        /// Tags (comma-separated or repeated).
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Blocker references (comma-separated or repeated).
        #[arg(long = "blocked-by", value_delimiter = ',')]
        blocked_by: Vec<String>,
        /// Linked GitHub issue numbers (comma-separated or repeated).
        #[arg(long = "gh-issue", value_delimiter = ',')]
        gh_issue: Vec<String>,
        /// Inline `## Why` problem statement.
        #[arg(long)]
        why: Option<String>,
        /// Inline `## Done When` pass criteria bullet(s) (repeatable).
        #[arg(long = "done-when")]
        done_when: Vec<String>,
        /// Inline `## References` bullet(s) (repeatable).
        #[arg(long)]
        references: Vec<String>,
        /// Body markdown source: a file path, or "-" to read from stdin.
        #[arg(long = "body-file")]
        body_file: Option<String>,
        /// Skip opening $EDITOR after creation.
        #[arg(long)]
        no_edit: bool,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Approve proposed (or backlog) task(s) into `todo`, enforcing pass criteria (`priority`, `size`, `## Why`, `## Done When`).
    Approve {
        /// Task ID(s) to approve.
        #[arg(required = true)]
        ids: Vec<String>,
        /// Set or override priority level (p0|p1|p2|p3|p4) before checking pass criteria.
        #[arg(long)]
        priority: Option<String>,
        /// Set or override size estimate (s|m|l) before checking pass criteria.
        #[arg(long)]
        size: Option<String>,
        /// Set or override `## Why` section before checking pass criteria.
        #[arg(long)]
        why: Option<String>,
        /// Set or override `## Done When` pass criteria bullets (repeatable).
        #[arg(long = "done-when")]
        done_when: Vec<String>,
        /// Move into `backlog` (icebox) instead of `todo`.
        #[arg(long)]
        backlog: bool,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Reject proposed (or backlog) task(s), archiving them.
    Reject {
        /// Task ID(s) to reject.
        #[arg(required = true)]
        ids: Vec<String>,
        /// Optional rejection reason recorded under `## Rejected`.
        #[arg(long)]
        reason: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// List tasks, optionally filtered.
    #[command(alias = "ls")]
    List {
        /// Filter by status (proposed|backlog|todo|in-progress|done|archived).
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
        /// Filter by priority (p0|p1|p2|p3|p4).
        #[arg(long)]
        priority: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print a full task (frontmatter + body).
    Show {
        /// Task ID (full, partial, or with slug).
        id: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Open a task file in $EDITOR.
    Edit {
        /// Task ID.
        id: String,
    },

    /// Update an existing task's metadata or body sections without opening $EDITOR.
    Set {
        /// Task ID.
        id: String,
        /// Replace the task title.
        #[arg(long)]
        title: Option<String>,
        /// Set priority (p0|p1|p2|p3|p4, or "" to clear).
        #[arg(long)]
        priority: Option<String>,
        /// Set size (s|m|l, or "" to clear).
        #[arg(long)]
        size: Option<String>,
        /// Area/component labels (comma-separated or repeated). Replaces the existing list; "" clears it.
        #[arg(long, value_delimiter = ',')]
        area: Option<Vec<String>>,
        /// Tags (comma-separated or repeated). Replaces the existing list; "" clears it.
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Blocker references (comma-separated or repeated). Replaces the existing list; "" clears it.
        #[arg(long = "blocked-by", value_delimiter = ',')]
        blocked_by: Option<Vec<String>>,
        /// Append blocker reference(s) without replacing existing blockers.
        #[arg(long = "add-blocked-by", value_delimiter = ',')]
        add_blocked_by: Vec<String>,
        /// Remove specific blocker reference(s) from the existing list.
        #[arg(long = "remove-blocked-by", value_delimiter = ',')]
        remove_blocked_by: Vec<String>,
        /// Linked GitHub issue numbers (comma-separated or repeated). Replaces the existing list; "" clears it.
        #[arg(long = "gh-issue", value_delimiter = ',')]
        gh_issue: Option<Vec<String>>,
        /// Replace the `## Why` section in the task body.
        #[arg(long)]
        why: Option<String>,
        /// Replace the `## Done When` section in the task body (repeatable).
        #[arg(long = "done-when")]
        done_when: Vec<String>,
        /// Replace the `## References` section in the task body (repeatable).
        #[arg(long)]
        references: Vec<String>,
        /// Body markdown source: a file path, or "-" to read from stdin. Replaces the body wholesale.
        #[arg(long = "body-file")]
        body_file: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Remove one or more task files.
    #[command(alias = "rm")]
    Remove {
        /// Task IDs.
        #[arg(required = true)]
        ids: Vec<String>,
    },

    /// Mark a task in-progress. Without an ID, claims the top ready task (lock-protected).
    Claim {
        /// Task ID. Omit to auto-claim the top ready task.
        id: Option<String>,
        /// Sprint filter for auto-claim (e.g. s12).
        #[arg(long)]
        sprint: Option<String>,
        /// Replace an existing started_at and clear completion fields.
        #[arg(long)]
        restart: bool,
        /// Start timestamp, UTC RFC3339 preferred. Defaults to now.
        #[arg(long)]
        started_at: Option<String>,
        /// Output as JSON for agent/orchestrator consumption.
        #[arg(long)]
        json: bool,
    },

    /// Revert an in-progress task to todo, clearing started_at (inverse of claim).
    Unclaim {
        /// Task ID.
        id: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
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
        /// Output as JSON.
        #[arg(long)]
        json: bool,
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

    /// Human decision gates (file, list, show, resolve).
    Gate {
        #[command(subcommand)]
        command: GateCommands,
    },

    /// Sprint management sub-commands.
    Sprint {
        #[command(subcommand)]
        command: SprintCommands,
    },

    /// Validate the task graph, or one task by ID; exit 1 if errors are found.
    Check {
        /// Task ID to validate. Omit to validate the entire task graph.
        id: Option<String>,
        /// Walk sibling repositories and validate cross-repo references (stub; not yet implemented).
        #[arg(long)]
        cross_repo: bool,
        /// Enforce pass criteria (`priority`, `size`, `## Why`, `## Done When`) on all todo/in-progress tasks.
        #[arg(long)]
        strict: bool,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show a summary: active tasks, proposed tasks, open gates, blocked tasks, and sprint progress.
    Status {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Built-in operator guide (workflow map, task authoring, proposals, implementation, gates, check).
    Guide {
        /// Guide topic: overview, add, propose, implement, gate, check.
        topic: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print the machine-readable schema for tasks, gates, sprints, or the CLI.
    Schema {
        /// Schema kind: task, gate, sprint, cli.
        kind: Option<String>,
        /// Output as JSON (default is pretty JSON).
        #[arg(long)]
        json: bool,
    },

    /// Update stint from crates.io through cargo install.
    Update,

    /// Print shell completion script to stdout.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum GateCommands {
    /// File a 7-section decision gate, wire it as a blocker on <blocked-task-id>, and unclaim <blocked-task-id>.
    File {
        /// ID of the stalled task that is blocked on this decision.
        blocked_task_id: String,
        /// Short title for the gate (`gate: ` prefix added automatically).
        title: String,
        /// One-sentence question for the human (`## Ask`).
        #[arg(long)]
        ask: Option<String>,
        /// Concrete choice(s), recommended first (`## Options`; repeatable).
        #[arg(long)]
        options: Vec<String>,
        /// Criteria that make an answer sufficient to unblock (`## Good answer`).
        #[arg(long = "good-answer")]
        good_answer: Option<String>,
        /// Commands or steps for the human to run/review (`## Test`; defaults to "none").
        #[arg(long)]
        test: Option<String>,
        /// Condensed findings so far (`## Evidence`).
        #[arg(long)]
        evidence: Option<String>,
        /// Relevant task IDs, branches, PRs, or file:line paths (`## Links`; repeatable).
        #[arg(long)]
        links: Vec<String>,
        /// Full 7-section markdown file path (or "-" for stdin) instead of inline flags.
        #[arg(long = "body-file")]
        body_file: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// List open human decision gates and the tasks they block.
    #[command(alias = "ls")]
    List {
        /// Include resolved (`done`) and `archived` gates.
        #[arg(long)]
        all: bool,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Inspect a decision gate and the tasks it blocks.
    Show {
        /// Gate task ID.
        id: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Record the human decision in `## Decision`, mark the gate done, and unblock waiting tasks.
    Resolve {
        /// Gate task ID.
        id: String,
        /// Decision text to write into `## Decision`.
        #[arg(long)]
        decision: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
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
            cmds::cmd_status(&repo)?;
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

        Some(Commands::Add {
            title,
            priority,
            size,
            area,
            tags,
            blocked_by,
            gh_issue,
            why,
            done_when,
            references,
            body_file,
            no_edit,
            backlog,
            proposed,
            json,
        }) => {
            if backlog && proposed {
                anyhow::bail!("--backlog and --proposed cannot be used together");
            }
            let repo = find_repo()?;
            let edits = cmds::TaskFieldEdits {
                area: Some(area),
                tags: Some(tags),
                blocked_by: Some(blocked_by),
                gh_issue: Some(gh_issue),
                body_source: body_file,
            };
            let status = if proposed {
                TaskStatus::Proposed
            } else if backlog {
                TaskStatus::Backlog
            } else {
                TaskStatus::Todo
            };
            let opts = cmds::AddTaskOptions {
                title: &title,
                priority: priority.as_deref(),
                size: size.as_deref(),
                edits: &edits,
                why: why.as_deref(),
                done_when: &done_when,
                references: &references,
                no_edit: no_edit || json,
                status: status.clone(),
            };
            let path = cmds::cmd_add_ext(&repo, &opts)?;
            let task = repo.read_task(&path)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "id": task.frontmatter.id,
                        "title": task.frontmatter.title,
                        "status": status.as_str(),
                        "path": path.to_string_lossy(),
                    }))
                    .unwrap()
                );
            } else {
                println!("{}", path.display());
            }
        }

        Some(Commands::Propose {
            title,
            priority,
            size,
            area,
            tags,
            blocked_by,
            gh_issue,
            why,
            done_when,
            references,
            body_file,
            no_edit,
            json,
        }) => {
            let repo = find_repo()?;
            let edits = cmds::TaskFieldEdits {
                area: Some(area),
                tags: Some(tags),
                blocked_by: Some(blocked_by),
                gh_issue: Some(gh_issue),
                body_source: body_file,
            };
            let opts = cmds::AddTaskOptions {
                title: &title,
                priority: priority.as_deref(),
                size: size.as_deref(),
                edits: &edits,
                why: why.as_deref(),
                done_when: &done_when,
                references: &references,
                no_edit: no_edit || json,
                status: TaskStatus::Proposed,
            };
            let path = cmds::cmd_add_ext(&repo, &opts)?;
            let task = repo.read_task(&path)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "id": task.frontmatter.id,
                        "title": task.frontmatter.title,
                        "status": "proposed",
                        "path": path.to_string_lossy(),
                    }))
                    .unwrap()
                );
            } else {
                println!("proposed: {}", path.display());
            }
        }

        Some(Commands::Approve {
            ids,
            priority,
            size,
            why,
            done_when,
            backlog,
            json,
        }) => {
            let repo = find_repo()?;
            let paths = cmds::cmd_approve(
                &repo,
                &ids,
                priority.as_deref(),
                size.as_deref(),
                why.as_deref(),
                &done_when,
                backlog,
            )?;
            if json {
                let items: Vec<_> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "approved": ids,
                        "status": if backlog { "backlog" } else { "todo" },
                        "paths": items,
                    }))
                    .unwrap()
                );
            } else {
                for path in paths {
                    println!("approved: {}", path.display());
                }
            }
        }

        Some(Commands::Reject { ids, reason, json }) => {
            let repo = find_repo()?;
            let paths = cmds::cmd_reject(&repo, &ids, reason.as_deref())?;
            if json {
                let items: Vec<_> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "rejected": ids,
                        "status": "archived",
                        "paths": items,
                    }))
                    .unwrap()
                );
            } else {
                for path in paths {
                    println!("rejected: {}", path.display());
                }
            }
        }

        Some(Commands::List {
            status,
            all,
            blocked,
            hide_blocked,
            sprint,
            area,
            tag,
            priority,
            json,
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
                priority.as_deref(),
            )?;
            if json {
                cmds::print_list_json(&rows);
            } else {
                cmds::print_list(&rows);
            }
        }

        Some(Commands::Show { id, json }) => {
            let repo = find_repo()?;
            if json {
                cmds::print_show_json(&repo, &id)?;
            } else {
                cmds::cmd_show(&repo, &id)?;
            }
        }

        Some(Commands::Edit { id }) => {
            let repo = find_repo()?;
            cmds::cmd_edit(&repo, &id)?;
        }

        Some(Commands::Set {
            id,
            title,
            priority,
            size,
            area,
            tags,
            blocked_by,
            add_blocked_by,
            remove_blocked_by,
            gh_issue,
            why,
            done_when,
            references,
            body_file,
            json,
        }) => {
            let repo = find_repo()?;
            let opts = cmds::SetTaskOptions {
                edits: cmds::TaskFieldEdits {
                    area,
                    tags,
                    blocked_by,
                    gh_issue,
                    body_source: body_file,
                },
                title,
                priority,
                size,
                add_blocked_by,
                remove_blocked_by,
                why,
                done_when,
                references,
            };
            let path = cmds::cmd_set_ext(&repo, &id, &opts)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "updated": id,
                        "path": path.to_string_lossy(),
                    }))
                    .unwrap()
                );
            } else {
                println!("{}", path.display());
            }
        }

        Some(Commands::Remove { ids }) => {
            let repo = find_repo()?;
            let paths = cmds::cmd_remove(&repo, &ids)?;
            for path in paths {
                println!("removed: {}", path.display());
            }
        }

        Some(Commands::Claim {
            id,
            sprint,
            restart,
            started_at,
            json,
        }) => {
            let repo = find_repo()?;
            cmds::cmd_claim(
                &repo,
                id.as_deref(),
                sprint.as_deref(),
                restart,
                started_at.as_deref(),
                json,
            )?;
        }

        Some(Commands::Unclaim { id, json }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_unclaim(&repo, &id)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "unclaimed": id,
                        "path": path.to_string_lossy(),
                    }))
                    .unwrap()
                );
            } else {
                println!("unclaimed: {}", path.display());
            }
        }

        Some(Commands::Done {
            id,
            actual,
            started_at,
            completed_at,
            json,
        }) => {
            let repo = find_repo()?;
            let path = cmds::cmd_done(
                &repo,
                &id,
                actual.as_deref(),
                started_at.as_deref(),
                completed_at.as_deref(),
            )?;
            let unblocked = cmds::tasks_unblocked_by_done(&repo, &id)?;
            let pruned = cmds::prune_done_blocker_refs(&repo, &id)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "done": id,
                        "path": path.to_string_lossy(),
                        "unblocked": unblocked.iter().map(|t| json!({"id": t.id, "title": t.title})).collect::<Vec<_>>(),
                        "pruned": pruned.iter().map(|t| json!({"id": t.id, "title": t.title})).collect::<Vec<_>>(),
                    }))
                    .unwrap()
                );
            } else {
                println!("done: {}", path.display());
                if !unblocked.is_empty() {
                    println!("unblocked:");
                    for task in unblocked {
                        println!("  {} {}", task.id, task.title);
                    }
                }
                if !pruned.is_empty() {
                    println!("pruned blockers:");
                    for task in pruned {
                        println!("  {} {}", task.id, task.title);
                    }
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
            json,
        }) => {
            let repo = find_repo()?;
            let report = cmds::cmd_next(
                &repo,
                sprint.as_deref(),
                include_area_conflicts,
                include_backlog,
            )?;
            if json {
                cmds::print_next_json(&repo, &report);
            } else {
                cmds::print_next(&report);
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

        Some(Commands::Gate { command }) => match command {
            GateCommands::File {
                blocked_task_id,
                title,
                ask,
                options,
                good_answer,
                test,
                evidence,
                links,
                body_file,
                json,
            } => {
                let repo = find_repo()?;
                let opts = cmds::GateFileOptions {
                    blocked_task_id: &blocked_task_id,
                    title: &title,
                    body_source: body_file.as_deref(),
                    ask: ask.as_deref(),
                    options: &options,
                    good_answer: good_answer.as_deref(),
                    test: test.as_deref(),
                    evidence: evidence.as_deref(),
                    links: &links,
                };
                let report = cmds::cmd_gate_file(&repo, &opts)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "gate_id": report.gate_id,
                            "gate_path": report.gate_path.to_string_lossy(),
                            "blocked_task_id": report.blocked_task_id,
                            "unclaimed": report.unclaimed,
                            "ask": report.ask,
                        }))
                        .unwrap()
                    );
                } else {
                    println!("[GATED] Stint {}", report.blocked_task_id);
                    println!("Gate:   {} ({})", report.gate_id, report.gate_path.display());
                    println!("Ask:    {}", report.ask);
                    println!(
                        "Blocks: {} now blocked_by {}{}",
                        report.blocked_task_id,
                        report.gate_id,
                        if report.unclaimed {
                            " (unclaimed -> todo)"
                        } else {
                            ""
                        }
                    );
                }
            }

            GateCommands::List { all, json } => {
                let repo = find_repo()?;
                let gates = cmds::cmd_gate_list(&repo, all)?;
                if json {
                    let payload: Vec<_> = gates
                        .iter()
                        .map(|g| {
                            json!({
                                "id": g.id,
                                "title": g.title,
                                "status": g.status,
                                "priority": g.priority,
                                "ask": g.ask,
                                "options": g.options,
                                "good_answer": g.good_answer,
                                "test": g.test,
                                "evidence": g.evidence,
                                "links": g.links,
                                "decision": g.decision,
                                "blocks": g.blocks,
                                "path": g.path.to_string_lossy(),
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                } else if gates.is_empty() {
                    println!("(no open gates)");
                } else {
                    for g in &gates {
                        let blocks = if g.blocks.is_empty() {
                            String::new()
                        } else {
                            format!(" [blocks: {}]", g.blocks.join(", "))
                        };
                        println!("{}  {}  ({}){}", g.id, g.title, g.status, blocks);
                        if let Some(ask) = &g.ask {
                            if !ask.is_empty() {
                                println!("      Ask: {}", ask.lines().next().unwrap_or(""));
                            }
                        }
                    }
                }
            }

            GateCommands::Show { id, json } => {
                let repo = find_repo()?;
                let g = cmds::cmd_gate_show(&repo, &id)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "id": g.id,
                            "title": g.title,
                            "status": g.status,
                            "priority": g.priority,
                            "ask": g.ask,
                            "options": g.options,
                            "good_answer": g.good_answer,
                            "test": g.test,
                            "evidence": g.evidence,
                            "links": g.links,
                            "decision": g.decision,
                            "blocks": g.blocks,
                            "path": g.path.to_string_lossy(),
                        }))
                        .unwrap()
                    );
                } else {
                    cmds::cmd_show(&repo, &id)?;
                    if !g.blocks.is_empty() {
                        println!("\nBlocks tasks: {}", g.blocks.join(", "));
                    }
                }
            }

            GateCommands::Resolve { id, decision, json } => {
                let repo = find_repo()?;
                let report = cmds::cmd_gate_resolve(&repo, &id, &decision)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "resolved": report.gate_id,
                            "path": report.gate_path.to_string_lossy(),
                            "decision": report.decision,
                            "unblocked": report.unblocked.iter().map(|t| json!({"id": t.id, "title": t.title})).collect::<Vec<_>>(),
                            "pruned": report.pruned.iter().map(|t| json!({"id": t.id, "title": t.title})).collect::<Vec<_>>(),
                        }))
                        .unwrap()
                    );
                } else {
                    println!("resolved gate {}: {}", report.gate_id, report.gate_path.display());
                    if !report.unblocked.is_empty() {
                        println!("unblocked:");
                        for task in report.unblocked {
                            println!("  {} {}", task.id, task.title);
                        }
                    }
                }
            }
        },

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

        Some(Commands::Check {
            id,
            cross_repo,
            strict,
            json,
        }) => {
            let repo = find_repo()?;
            let (errors, advisories) =
                cmds::cmd_check_ext(&repo, cross_repo, id.as_deref(), strict)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": errors.is_empty(),
                        "errors": errors,
                        "advisories": advisories,
                    }))
                    .unwrap()
                );
                if !errors.is_empty() {
                    process::exit(1);
                }
            } else if errors.is_empty() {
                println!("ok");
            } else {
                for e in &errors {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }

        Some(Commands::Status { json }) => {
            let repo = find_repo()?;
            if json {
                cmds::print_status_json(&repo)?;
            } else {
                cmds::cmd_status(&repo)?;
            }
        }

        Some(Commands::Guide { topic, json }) => {
            if json {
                let val = guide_json(topic.as_deref()).map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                let text = format_guide(topic.as_deref()).map_err(|e| anyhow::anyhow!("{}", e))?;
                print!("{}", text);
            }
        }

        Some(Commands::Schema { kind, json }) => {
            if json {
                let val = schema_json(kind.as_deref()).map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                let text = format_schema(kind.as_deref()).map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("{}", text);
            }
        }

        Some(Commands::Update) => {
            cmd_update()?;
        }

        Some(Commands::Completions { shell }) => {
            generate(shell, &mut Cli::command(), "stint", &mut std::io::stdout());
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
