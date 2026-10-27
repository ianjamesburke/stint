# Stint — Product Requirements Document

## Overview

Stint is a Rust CLI for task and sprint tracking as markdown files in your repo. All state lives as individual markdown files with YAML frontmatter inside a `.stint/` directory at the root of any git repository.

---

## Architecture

### Repo layout

```
<project-root>/
  .stint/
    tasks/
      0001-auth-middleware.md
      0002-tui-skeleton.md
      ...
    sprints/
      s01.md
      s02.md
      ...
    config.toml
```

### Crate layout

Single crate with both `[lib]` and `[[bin]]` targets:

- `src/lib.rs` — library: schema, parsing, validation, mutation, check
- `src/main.rs` — binary: Clap CLI, thin shell over the library

---

## Task File Format

Each task is a single markdown file. Filename: `<id>-<slug>.md`.

### Frontmatter schema

```yaml
---
id: "0001"                        # required, zero-padded 4-digit string
title: "Auth middleware"          # required, string
status: backlog                   # required, enum: proposed|backlog|todo|in-progress|done|archived
priority: p2                      # optional (required for todo pipeline), enum: p0|p1|p2|p3|p4 (p0 = highest)
size: m                           # optional (required for todo pipeline), enum: s|m|l
estimate: "4h"                    # optional, string (e.g. "2h", "30m", "1.5h")
actual: "0h"                      # optional, string, time logged so far
created_at: "2026-06-14T12:00:00Z" # optional, RFC3339 UTC timestamp
started_at: "2026-06-14T13:00:00Z" # optional, RFC3339 UTC timestamp
completed_at: "2026-06-14T15:00:00Z" # optional, RFC3339 UTC timestamp
blocked_by: []                    # optional, unified blocker list (see Blocker Model)
gh_issue: []                      # optional, list OR single int/string of GH issue numbers
area: []                          # optional, list OR string of area labels
tags: []                          # optional, list OR string of arbitrary tags
---
```

**Coercion rule:** `blocked_by`, `gh_issue`, `area`, `tags` accept either a single scalar value or a list. The library normalizes all of these internally at parse time. `stint check` validates types after coercion.

### Body & Pass Criteria Contract

Tasks in the active pipeline (`todo` / `in-progress`, promoted via `stint approve` or validated via `stint check --strict`) require `priority`, `size`, and two structured markdown sections:

```markdown
## Why

Why this task exists, what problem it solves, and how it serves `NORTH_STAR.md`.

## Done When

- [ ] Concrete, checkable pass criterion 1
- [ ] Concrete, checkable pass criterion 2

## References

- `src/auth/middleware.rs:142` — relevant code location
- owner/repo#847 — upstream dependency
```

### Decision Gate Contract (`tags: [gate]`)

Human decision gates are tasks tagged `gate` (filed in `status: backlog` via `stint gate file` so `stint next` never schedules them as agent work). Every gate must include all 7 sections:
`## Ask`, `## Options`, `## Good answer`, `## Test`, `## Evidence`, `## Links`, and `## Decision` (populated when resolved via `stint gate resolve`).

---

## Sprint File Format

```markdown
# Sprint 12 · Jun 9–20 · goal: ship TUI skeleton

- [0001](../tasks/0001-auth-middleware.md)
- [0004](../tasks/0004-tui-skeleton.md)
- [0007](../tasks/0007-gh-import.md)
- [0003](../tasks/0003-docs.md)
```

- First line: `# Sprint <id> · <date range> · goal: <goal>` (parsed by core)
- Remaining lines: ordered task entries (line order = priority order)
- Each entry is a markdown link (`[<id>](../tasks/<id>-<slug>.md)`) — a real
  clickable link: cmd-click in VS Code, `gf`/link-follow in Vim. `stint sprint
  add` writes this form.
- The task ID is extracted regardless of form — bare IDs (`0001`), slug forms
  (`0001-auth-middleware`), plain paths, and markdown links are all accepted

---

## CLI Commands

### Task & Proposal management

| Command | Description |
|---|---|
| `stint add "Title" [--priority p1] [--size m] [--why ...] [--done-when ...] [--references ...] [--proposed] [--json]` | Create a new task or proposal (skips `$EDITOR` automatically when inline body flags or `--json` are supplied) |
| `stint propose "Title" [--why ...] [--done-when ...] [--json]` | File a throwaway PRD / candidate idea in `status: proposed` outside the active pipeline |
| `stint approve <id>... [--priority p1] [--size m] [--why ...] [--done-when ...] [--json]` | Validate pass criteria (`priority`, `size`, `## Why`, `## Done When`) and promote `proposed`/`backlog` task(s) to `todo` |
| `stint reject <id>... [--reason "..."] [--json]` | Reject and archive a `proposed` (or `backlog`) task |
| `stint list [--status ...] [--all] [--blocked] [--hide-blocked] [--json]` | List tasks (filterable by status, sprint, area, tag, priority) |
| `stint show <id> [--json]` | Print full task (frontmatter, computed state, extracted sections, and body) |
| `stint edit <id>` | Open task file in `$EDITOR` |
| `stint set <id> [--title ...] [--priority ...] [--size ...] [--area ...] [--tags ...] [--blocked-by ...] [--add-blocked-by ...] [--remove-blocked-by ...] [--why ...] [--done-when ...] [--references ...] [--body-file ...] [--json]` | Headlessly mutate frontmatter and/or markdown sections without opening `$EDITOR` |
| `stint next [--sprint sN] [--include-backlog] [--json]` | Show ready tasks ordered by priority + age (excluding area conflicts with `in-progress` tasks) |
| `stint claim [<id>] [--json]` | Mark task `in-progress` and set `started_at` (omit `<id>` to auto-claim the top ready task) |
| `stint unclaim <id> [--json]` | Revert an `in-progress` task to `todo`, clearing `started_at` |
| `stint done <id> [<actual>] [--json]` | Mark task `done`, record actual time, and prune completed blocker from dependent tasks |
| `stint log <id> <time>` | Add time to `actual` (e.g. `stint log 0001 2h`) |
| `stint archive <id>` | Move task to `archived` status |

### Decision Gate management (`stint gate`)

| Command | Description |
|---|---|
| `stint gate file <blocked_task_id> "<title>" --ask ... --options ... --good-answer ... --evidence ... [--test ...] [--links ...] [--json]` | Atomically file a 7-section gate in `backlog`, append it to `<blocked_task_id>.blocked_by`, and unclaim `<blocked_task_id>` if `in-progress` |
| `stint gate list [--all] [--json]` | List open (or all) human decision gates and which tasks they block |
| `stint gate show <gate_id> [--json]` | Display a gate's parsed 7 sections and blocked task IDs |
| `stint gate resolve <gate_id> --decision "..." [--json]` | Record the human decision in `## Decision`, mark the gate `done` (`0m`), and prune it from blocked tasks |

### Built-in Guide, Schema & Validation

| Command | Description |
|---|---|
| `stint guide [overview\|add\|propose\|implement\|gate\|check] [--json]` | Built-in operator guide (replaces external agent skill files) |
| `stint schema [task\|gate\|sprint\|cli] [--json]` | Built-in frontmatter, section, and CLI schema reference |
| `stint check [<id>] [--strict] [--cross-repo] [--json]` | Validate task graph (with `--strict`, enforces pass criteria on all `todo`/`in-progress` tasks) |
| `stint status [--json]` | Summary: active, proposed, backlog, open gates, blocked tasks, sprint progress |

### Sprint management

| Command | Description |
|---|---|
| `stint sprint new <id> <date-range>` | Create a new sprint index file |
| `stint sprint list` | List all sprints |
| `stint sprint show <id>` | Show sprint with task summaries |
| `stint sprint add <sprint-id> <task-id>` | Append task to sprint |
| `stint sprint remove <sprint-id> <task-id>` | Remove task from sprint |
| `stint sprint reorder <id>` | Interactive reorder (uses `$EDITOR`) |

---

## `stint check` Validation Rules

1. All required fields present (`id`, `title`, `status`)
2. `status` is a valid enum value (`proposed`, `backlog`, `todo`, `in-progress`, `done`, `archived`)
3. `estimate` and `actual` are valid duration strings if present
4. `blocked_by` local task refs resolve to existing task files
5. `blocked_by` direct task-file paths resolve to existing, parseable task files whose `id` matches the filename prefix
6. `blocked_by` external refs are structurally valid (format check only)
7. Sprint index files reference only existing task IDs
8. No circular `blocked_by` references
9. `id` matches the numeric prefix of the filename
10. No duplicate IDs across all task files
11. No `in-progress`/`done` task has an unresolved local-task blocker (only `proposed`/`backlog`/`todo` tasks may carry active blockers)
12. Timestamp fields (`created_at`, `started_at`, `completed_at`) are valid RFC3339 if present
13. `started_at` and `completed_at` are required on `done` tasks (`started_at <= completed_at`)
14. `started_at` is required on `in-progress` tasks
15. Tasks tagged `gate` must contain non-empty `## Ask`, `## Options`, `## Good answer`, `## Test`, `## Evidence`, and `## Links` sections, plus a `## Decision` heading (non-empty when `status: done`)
16. With `--strict` (and on `stint approve`), `todo` and `in-progress` tasks must have `priority`, `size`, non-empty `## Why`, and non-empty `## Done When`

---

## Blocker Model

`blocked_by` is a single unified field. Type is inferred by syntax at parse time:

| Syntax | Meaning | Validated |
|---|---|---|
| bare integer or all-digit string | local stint task (zero-padded) | yes — must resolve to existing task |
| `@N` | local GitHub issue | no |
| `./path/.stint/tasks/NNNN-slug.md` or `../path/.stint/tasks/NNNN-slug.md` | direct local task file path | yes — must exist, parse, and match `NNNN` |
| `owner/repo@N` | external GitHub issue | format only |
| `owner/repo:NNNN` | task in external GitHub repo | format only |
| `../path:NNNN` | task in sibling local directory | format only |
| `../path@N` | issue in sibling local directory | format only |
| quoted string | free-text note | no |

`stint status` renders all active blocker types in a unified list. Done
local-task blockers are ignored. Direct task-file blockers are ignored when the
target task is `done` or `archived`; missing, malformed, and non-done target
files remain active blockers.

`stint check --cross-repo` remains unrelated/deferred. Normal `stint check`
resolves explicit direct task-file paths, but it does not walk sibling repos or
resolve `../path:NNNN`, `owner/repo:NNNN`, or GitHub issue refs.

---

## Time Tracking

- `estimate`: set at task creation, stored as string ("4h", "30m", "1.5h")
- `actual`: accumulated via `stint log <id> <duration>`, stored as string
- `stint status` computes sprint-level: committed hours, logged hours, remaining
- Duration parsing: `h` for hours, `m` for minutes, decimals allowed ("1.5h")

---

## GitHub Integration

Optional. When `gh` CLI is available and repo has a GitHub remote:

- `stint gh import` pulls issue title, body, labels into task frontmatter + body
- `gh_issue` frontmatter stores the issue number(s)
- A task can map to multiple issues (bundle) or one issue can split into multiple tasks
- `stint gh sync` writes task status back as a GH issue comment (not closing the issue)

---

## Config

`.stint/config.toml`:

```toml
[project]
name = "My Project"
default_sprint = "s12"   # sprint to add new tasks to by default

[gh]
repo = "owner/repo"      # optional, inferred from git remote if absent
```

---

## Done Criteria for v1

- [ ] `stint add`, `list`, `show`, `edit`, `done`, `log`, `archive` all work
- [ ] `stint sprint new`, `list`, `show`, `add`, `remove` all work
- [ ] `stint check` validates all 11 rules
- [ ] `stint status` renders blocked summary and sprint progress
- [ ] String/list coercion works for all polymorphic fields
- [ ] All core logic has unit tests
- [ ] Library modules have no I/O side effects
- [ ] `src/main.rs` is a thin shell — no business logic
- [ ] Builds cleanly with `cargo build`
