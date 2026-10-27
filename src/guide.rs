//! Built-in operator guide and schema reference (`stint guide` and `stint schema`).
//!
//! Distills all operational rules for authoring tasks, proposing candidate work,
//! implementing stints, and filing/resolving human decision gates directly into
//! the CLI so agents and humans never need external skill files to drive `stint`.

use serde_json::{json, Value};

use crate::mutate::REQUIRED_GATE_SECTIONS;

/// Available guide topics.
pub const GUIDE_TOPICS: &[(&str, &str)] = &[
    (
        "overview",
        "Workflow map, status lifecycle, and core invariants",
    ),
    (
        "add",
        "Authoring actionable tasks with priority, size, and pass criteria",
    ),
    (
        "propose",
        "Filing throwaway PRDs or candidate ideas in the proposed state",
    ),
    (
        "implement",
        "Picking, claiming, implementing, batching, and closing out stints",
    ),
    (
        "gate",
        "Filing and resolving human decision gates when blocked",
    ),
    (
        "check",
        "Validating the task graph and enforcing pass criteria",
    ),
];

/// Available schema kinds.
pub const SCHEMA_KINDS: &[(&str, &str)] = &[
    (
        "task",
        "Task frontmatter fields, blocker syntax, and markdown body contract",
    ),
    (
        "gate",
        "Decision gate frontmatter and 7-section markdown contract",
    ),
    ("sprint", "Sprint index file format"),
    ("cli", "Command overview and JSON automation contract"),
];

/// Return structured JSON for `stint guide [topic]`.
pub fn guide_json(topic: Option<&str>) -> Result<Value, String> {
    match topic.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("overview") => Ok(json!({
            "topic": "overview",
            "summary": "Stint tracks tasks, proposals, sprints, and decision gates as markdown files inside .stint/.",
            "lifecycle": {
                "proposed": "Candidate idea or throwaway PRD outside the active pipeline. Ignored by `stint next` and `stint claim`. Promoted via `stint approve <id>` (which enforces pass criteria) or discarded via `stint reject <id>`.",
                "backlog": "Iceboxed work or open human decision gates (`tags: [gate]`). Excluded from `stint next` by default.",
                "todo": "Committed, actionable work in the ready pool (or waiting on `blocked_by`).",
                "in-progress": "Actively claimed by an agent or human (`stint claim`).",
                "done": "Completed and verified (`stint done`). Automatically unblocks dependent tasks.",
                "archived": "Retired or rejected work (`stint archive` or `stint reject`)."
            },
            "core_loop": [
                "1. Discover ready work: `stint next --json` (or `stint status --json`).",
                "2. Claim before editing code: `stint claim <id> --json` (or `stint claim` to auto-claim the top ready task under a lock).",
                "3. Implement and verify every bullet in `## Done When`.",
                "4. If blocked on a human decision, never guess or stall: run `stint gate file <id> \"<ask>\" ...` (creates gate, wires `blocked_by`, and unclaims `<id>`).",
                "5. If you spot out-of-scope follow-up work, file it without polluting the ready queue: `stint propose \"<title>\" --why \"...\"`.",
                "6. Close out after verification/merge: `stint done <id> --actual 30m` and `stint check`."
            ],
            "topics": GUIDE_TOPICS.iter().map(|(k, d)| json!({"topic": k, "description": d})).collect::<Vec<_>>(),
        })),
        Some("add") | Some("create") => Ok(json!({
            "topic": "add",
            "summary": "Create committed, immediately actionable tasks in `todo` status without opening $EDITOR.",
            "rules": [
                "North Star alignment: verify the task serves the repository's goals and state why in `## Why`.",
                "Always set `--priority p0..p4` (`p0` highest, `p4` lowest) and `--size s|m|l`.",
                "Do not set hour estimates unless the repo explicitly uses sprint hour tracking; prefer `--size s|m|l`.",
                "Every actionable task must have a concrete `## Why` and checkable pass criteria in `## Done When`.",
                "Model real sequencing dependencies with `--blocked-by <id>`, not sprints.",
                "When `--why`, `--done-when`, `--body-file`, `--no-edit`, or `--json` is passed (or stdout is not a TTY), `stint add` never opens `$EDITOR`."
            ],
            "examples": [
                "stint add \"Fix token refresh race\" --priority p1 --size s --area auth --why \"Concurrent requests drop the refresh token\" --done-when \"Refresh is mutex-guarded\" --done-when \"cargo test passes\"",
                "stint add \"Add export command\" --priority p2 --size m --body-file /tmp/body.md --no-edit"
            ]
        })),
        Some("propose") | Some("proposals") | Some("draft") => Ok(json!({
            "topic": "propose",
            "summary": "Capture throwaway PRDs, rough ideas, or agent-discovered side-quests in `proposed` status outside the active stint pipeline.",
            "rules": [
                "`stint propose \"<title>\"` (or `stint add --proposed`) creates a task with `status: proposed`.",
                "`proposed` tasks are never scheduled by `stint next` or `stint claim`, and are hidden from default `stint list` unless `--status proposed` or `--all` is passed.",
                "Promoting a proposal into the active pipeline via `stint approve <id>` enforces the full stint contract: `priority`, `size`, `## Why`, and `## Done When` must all be present (you can supply missing fields on the `stint approve` command line).",
                "Rejecting a proposal via `stint reject <id> [--reason \"...\"]` moves it to `archived`."
            ],
            "examples": [
                "stint propose \"Explore WebSocket sync\" --area sync --why \"Polling adds 500ms latency\"",
                "stint list --status proposed",
                "stint approve 0018 --priority p2 --size m --done-when \"Benchmark shows <50ms latency\"",
                "stint reject 0018 --reason \"Out of scope for v1\""
            ]
        })),
        Some("implement") | Some("work") | Some("run") => Ok(json!({
            "topic": "implement",
            "summary": "End-to-end execution workflow for claiming, implementing, batching, and closing out stints.",
            "phases": [
                "1. Pick: run `stint next --json` on the canonical planning branch to find the highest-priority unblocked task.",
                "2. Claim immediately: run `stint claim <id>` as the first state mutation before starting implementation.",
                "3. Isolate: create or resume an implementation branch/worktree (`feature/stint-<id>-<slug>`).",
                "4. Formulate & Implement: read `stint show <id>`, satisfy every criterion in `## Done When`, and run the repository's test suite.",
                "5. Blocked?: if pre-flight fails before work begins, run `stint unclaim <id>`. If blocked mid-implementation on a human decision, run `stint gate file <id> \"<ask>\" ...` (which files the gate, wires `blocked_by`, and unclaims `<id>`).",
                "6. Closeout: after verification/merge, run `stint done <id> --actual <duration>` and `stint check`."
            ],
            "batching": [
                "Small, tightly related `s`/`m` tasks in the same area may be implemented in one branch/PR.",
                "Claim every task in the batch (`stint claim <id>`) before starting work.",
                "Run `stint done <id>` for each task in the batch upon closeout."
            ]
        })),
        Some("gate") | Some("gates") => Ok(json!({
            "topic": "gate",
            "summary": "Convert a human decision blocker into a structured, routable `gate` artifact that automatically unblocks work when answered.",
            "when_to_file": [
                "The next step requires an architectural, product, or spend/priority decision that belongs to the human.",
                "Required credentials, specs, or external approvals are missing.",
                "You are about to pause or hand off a claimed run for any reason."
            ],
            "required_sections": REQUIRED_GATE_SECTIONS,
            "workflow": [
                "File a gate: `stint gate file <blocked-task-id> \"<short ask>\" --ask \"...\" --options \"(recommended) Option A: ...\" --options \"Option B: ...\" --good-answer \"...\" --test \"none\" --evidence \"...\" --links \"...\"` (or pass `--body-file`).",
                "`stint gate file` validates all 7 sections, creates the gate task tagged `gate` in `backlog`, appends its ID to `<blocked-task-id>`'s `blocked_by` without clobbering existing blockers, and unclaims `<blocked-task-id>` if it was `in-progress`.",
                "List open gates: `stint gate list [--json]`.",
                "Resolve a gate: `stint gate resolve <gate-id> --decision \"Approved Option A\"` writes `## Decision`, marks the gate `done`, prunes it from blocked tasks, and reports newly unblocked tasks."
            ]
        })),
        Some("check") | Some("validate") => Ok(json!({
            "topic": "check",
            "summary": "Validate task graph integrity, blocker resolution, gate structure, and pass criteria.",
            "rules": [
                "Required frontmatter fields (`id`, `title`, `status`), valid enums, and RFC3339 timestamps.",
                "`id` matches the 4-digit filename prefix and is unique across `.stint/tasks/`.",
                "All local `blocked_by` task IDs and sprint index entries resolve to existing tasks with no dependency cycles.",
                "No `in-progress` or `done` task may carry unresolved local-task blockers.",
                "Every task tagged `gate` must contain all 7 required gate sections (`Ask`, `Options`, `Good answer`, `Test`, `Evidence`, `Links`, `Decision`), and `Decision` must be non-empty when the gate is `done`.",
                "`stint check --strict` additionally verifies that every `todo` and `in-progress` task has `priority`, `size`, `## Why`, and `## Done When`."
            ],
            "commands": [
                "stint check",
                "stint check <id>",
                "stint check --strict --json"
            ]
        })),
        Some(other) => Err(format!(
            "unknown guide topic {:?}; expected one of: overview, add, propose, implement, gate, check",
            other
        )),
    }
}

/// Format `stint guide [topic]` as human-readable text.
pub fn format_guide(topic: Option<&str>) -> Result<String, String> {
    let val = guide_json(topic)?;
    let topic_name = val["topic"].as_str().unwrap_or("overview");
    let summary = val["summary"].as_str().unwrap_or("");
    let mut out = format!("# stint guide: {topic_name}\n\n{summary}\n");

    if let Some(lifecycle) = val.get("lifecycle").and_then(Value::as_object) {
        out.push_str("\n## Lifecycle States\n");
        for (state, desc) in lifecycle {
            out.push_str(&format!(
                "  {:<12} {}\n",
                state,
                desc.as_str().unwrap_or("")
            ));
        }
    }

    for (key, heading) in [
        ("core_loop", "Core Loop"),
        ("rules", "Rules"),
        ("phases", "Execution Phases"),
        ("batching", "Batching"),
        ("when_to_file", "When to File a Gate"),
        ("required_sections", "Required Gate Sections"),
        ("workflow", "Workflow"),
        ("examples", "Examples"),
        ("commands", "Commands"),
    ] {
        if let Some(items) = val.get(key).and_then(Value::as_array) {
            out.push_str(&format!("\n## {heading}\n"));
            for item in items {
                if let Some(s) = item.as_str() {
                    out.push_str(&format!("  - {s}\n"));
                }
            }
        }
    }

    if let Some(topics) = val.get("topics").and_then(Value::as_array) {
        out.push_str("\n## Topics (`stint guide <topic>`)\n");
        for item in topics {
            let t = item["topic"].as_str().unwrap_or("");
            let d = item["description"].as_str().unwrap_or("");
            out.push_str(&format!("  {:<12} {}\n", t, d));
        }
    }

    Ok(out)
}

/// Return structured JSON for `stint schema [kind]`.
pub fn schema_json(kind: Option<&str>) -> Result<Value, String> {
    let task_schema = json!({
        "filename": "<4-digit-id>-<slug>.md in .stint/tasks/",
        "frontmatter": {
            "id": {"type": "string", "required": true, "format": "zero-padded 4-digit string (e.g. \"0001\")"},
            "title": {"type": "string", "required": true},
            "status": {"type": "enum", "required": true, "values": ["proposed", "backlog", "todo", "in-progress", "done", "archived"]},
            "priority": {"type": "enum", "required_for_pipeline": true, "values": ["p0", "p1", "p2", "p3", "p4"]},
            "size": {"type": "enum", "required_for_pipeline": true, "values": ["s", "m", "l"]},
            "estimate": {"type": "duration", "required": false, "format": "Nh or Nm (e.g. \"2h\", \"30m\", \"1.5h\")"},
            "actual": {"type": "duration", "required": false, "format": "Nh or Nm"},
            "created_at": {"type": "timestamp", "required": false, "format": "RFC3339 UTC"},
            "started_at": {"type": "timestamp", "required": false, "format": "RFC3339 UTC"},
            "completed_at": {"type": "timestamp", "required": false, "format": "RFC3339 UTC"},
            "blocked_by": {
                "type": "list<blocker>",
                "coercion": "scalar or list",
                "variants": [
                    "NNNN or int -> local stint task",
                    "@N -> local GitHub issue",
                    "../path/.stint/tasks/NNNN-slug.md -> direct task file path",
                    "../path:NNNN -> sibling repo stint task",
                    "owner/repo:NNNN -> external repo stint task",
                    "owner/repo@N -> external GitHub issue",
                    "free-text string -> note blocker"
                ]
            },
            "gh_issue": {"type": "list<string>", "coercion": "scalar or list"},
            "area": {"type": "list<string>", "coercion": "scalar or list"},
            "tags": {"type": "list<string>", "coercion": "scalar or list"}
        },
        "body_sections": {
            "Why": {"required_for_pipeline": true, "description": "Problem statement and North Star connection"},
            "Done When": {"required_for_pipeline": true, "description": "Checkable pass criteria bullets"},
            "References": {"required_for_pipeline": false, "description": "Relevant file:line locations, issues, or PRs"}
        }
    });

    let gate_schema = json!({
        "tag": "gate",
        "status_when_open": "backlog",
        "status_when_resolved": "done",
        "required_sections": [
            {"heading": "## Ask", "required_on_file": true, "description": "One sentence — the exact question for the human."},
            {"heading": "## Options", "required_on_file": true, "description": "2-4 concrete choices, recommended option FIRST and labelled (recommended)."},
            {"heading": "## Good answer", "required_on_file": true, "description": "Criteria that make an answer sufficient to unblock."},
            {"heading": "## Test", "required_on_file": true, "description": "Exact commands or steps to review, or 'none'."},
            {"heading": "## Evidence", "required_on_file": true, "description": "Condensed findings so the human doesn't have to redo discovery."},
            {"heading": "## Links", "required_on_file": true, "description": "Task IDs, branch, PR, file:line paths."},
            {"heading": "## Decision", "required_on_file": false, "required_on_done": true, "description": "Filled when resolving the gate (`stint gate resolve <id> --decision \"...\"`)."}
        ]
    });

    let sprint_schema = json!({
        "filename": "s<N>.md in .stint/sprints/",
        "header": "# Sprint <N> · <date range> · goal: <goal>",
        "entries": "Ordered markdown list `- [<id>](../tasks/<id>-<slug>.md)`"
    });

    let cli_schema = json!({
        "commands": [
            {"cmd": "stint status [--json]", "description": "Summary of active, proposed, backlog, open gates, blocked tasks, and sprint progress"},
            {"cmd": "stint next [--sprint sN] [--include-backlog] [--json]", "description": "Next claimable tasks ordered by priority and age, excluding area conflicts"},
            {"cmd": "stint add <title> [--priority p0..p4] [--size s|m|l] [--why ...] [--done-when ...] [--proposed] [--json]", "description": "Create a new task or proposal"},
            {"cmd": "stint propose <title> [--why ...] [--done-when ...] [--json]", "description": "Create a candidate task in `proposed` status"},
            {"cmd": "stint approve <id>... [--priority p0..p4] [--size s|m|l] [--done-when ...] [--json]", "description": "Validate pass criteria and promote proposed/backlog task(s) to todo"},
            {"cmd": "stint reject <id>... [--reason ...] [--json]", "description": "Reject and archive a proposed task"},
            {"cmd": "stint claim [id] [--sprint sN] [--restart] [--json]", "description": "Claim a specific task or auto-claim the top ready task under an advisory lock"},
            {"cmd": "stint unclaim <id> [--json]", "description": "Revert an in-progress task to todo and clear started_at"},
            {"cmd": "stint done <id> [--actual <duration>] [--json]", "description": "Mark task done, prune it from dependent tasks' blocked_by, and report unblocked tasks"},
            {"cmd": "stint set <id> [--priority ...] [--size ...] [--add-blocked-by ...] [--remove-blocked-by ...] [--why ...] [--done-when ...]", "description": "Headlessly update task frontmatter or body sections"},
            {"cmd": "stint gate file <task-id> <ask-title> [--body-file ...] [--ask ...] [--options ...] [--json]", "description": "File a 7-section decision gate, wire it into <task-id>'s blocked_by, and unclaim <task-id>"},
            {"cmd": "stint gate list [--all] [--json]", "description": "List open human decision gates and the tasks they block"},
            {"cmd": "stint gate show <gate-id> [--json]", "description": "Inspect a gate and the tasks it blocks"},
            {"cmd": "stint gate resolve <gate-id> --decision <text> [--json]", "description": "Record the human decision, mark the gate done, and unblock waiting tasks"},
            {"cmd": "stint check [id] [--strict] [--json]", "description": "Validate the task graph, gate sections, and (with --strict) pass criteria"}
        ]
    });

    match kind.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") => Ok(json!({
            "kinds": SCHEMA_KINDS.iter().map(|(k, d)| json!({"kind": k, "description": d})).collect::<Vec<_>>(),
            "task": task_schema,
            "gate": gate_schema,
            "sprint": sprint_schema,
            "cli": cli_schema,
        })),
        Some("task") => Ok(task_schema),
        Some("gate") => Ok(gate_schema),
        Some("sprint") => Ok(sprint_schema),
        Some("cli") => Ok(cli_schema),
        Some(other) => Err(format!(
            "unknown schema kind {:?}; expected one of: task, gate, sprint, cli",
            other
        )),
    }
}

/// Format `stint schema [kind]` as pretty JSON (the canonical schema representation).
pub fn format_schema(kind: Option<&str>) -> Result<String, String> {
    let val = schema_json(kind)?;
    serde_json::to_string_pretty(&val).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_guide_topics_resolve() {
        assert!(guide_json(None).is_ok());
        for (topic, _) in GUIDE_TOPICS {
            let text = format_guide(Some(topic)).expect("topic should format");
            assert!(text.contains(topic));
        }
        assert!(guide_json(Some("nonexistent")).is_err());
    }

    #[test]
    fn all_schema_kinds_resolve() {
        assert!(schema_json(None).is_ok());
        for (kind, _) in SCHEMA_KINDS {
            let text = format_schema(Some(kind)).expect("schema should format");
            assert!(!text.is_empty());
        }
        assert!(schema_json(Some("nonexistent")).is_err());
    }
}
