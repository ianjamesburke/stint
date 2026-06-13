# stint

Git-tracked, markdown-backed sprint planning and task tracking.

Tasks are individual markdown files. Sprints are ordered lists. Everything lives in `.stint/` inside your repo — versioned, diffable, readable in any editor.

```
.stint/
  tasks/
    0001-auth-middleware.md
    0002-tui-skeleton.md
  sprints/
    s01.md
```

## Install

```bash
cargo install --git https://github.com/ianjamesburke/stint
```

## Usage

```bash
# Tasks
stint add "Implement auth middleware"
stint list
stint list --status in-progress
stint list --blocked
stint list --hide-blocked
stint list --sprint s1
stint show 1
stint next
stint next --claim
stint log 1 2h
stint done 1 --actual 3h
stint archive 1

# Sprints
stint sprint new s1 "Jun 9-20" --goal "ship v1"
stint sprint add s1 0001
stint sprint show s1
stint sprint reorder s1
stint sprint remove s1 0001

# Validation
stint check
stint status
```

## Next work

`stint next` derives claimable work from the task graph. It does not maintain a
separate parallel-work list.

- tasks must be `backlog` or `todo`
- local task blockers must be `done`
- all other blocker types (issues, external tasks, notes) make a task blocked
- sprint order is priority order
- tasks whose `area` overlaps with `in-progress` work are hidden by default
- `stint next --claim` marks the top ready task `in-progress`

Use `stint next --include-area-conflicts` to see ready tasks even if they touch
an area already in progress.

## Task format

```markdown
---
id: "0001"
title: Auth middleware
status: in-progress
estimate: 4h
actual: 2h
sprint: s1
blocked_by:
  - 2                          # local task 0002
  - @847                       # local GitHub issue #847
  - acme/sdk@847               # external GitHub issue
  - ianjamesburke/PLEXI:0146   # task in another GitHub repo
  - ../PLEXI:0146              # task in a sibling directory
  - "waiting for design"       # free-text note
gh_issue: [123]
area: [backend]
tags: []
---

## Why

Why this task exists and what problem it solves.

## Gotchas

Non-obvious constraints, prior attempts, things that will bite you.
```

`blocked_by` is a unified field — type is inferred by syntax. `gh_issue`, `area`, and `tags` all accept a single value or a list.

| Syntax | Meaning |
|---|---|
| bare integer | local stint task (auto-padded to 4 digits) |
| `@N` | local GitHub issue |
| `owner/repo@N` | external GitHub issue |
| `owner/repo:NNNN` | task in an external GitHub repo |
| `../path:NNNN` | task in a sibling local directory |
| `../path@N` | issue in a sibling local directory |
| quoted string | free-text blocker note |

## Sprint format

```markdown
# Sprint 1 · Jun 9–20 · goal: ship v1

- ../tasks/0001-auth-middleware.md
- ../tasks/0002-tui-skeleton.md
```

Line order is priority order. Entries are relative links to the task file, so
in an editor like Vim you can put the cursor on a line and press `gf` to jump
straight to the task. `stint sprint add` writes this form automatically; run
`stint sprint relink <id>` (or `--all`) to convert older bare-ID sprint files.

Bare IDs (`- 0001`) and slug forms (`- 0001-auth-middleware`) are still parsed —
the task ID is read from the entry regardless of form. Edit directly or use
`stint sprint reorder`.

## Validation

`stint check` validates the entire task graph:

- Required fields present
- Status is a valid enum value
- Duration strings are valid (`4h`, `30m`, `1.5h`)
- `blocked_by` local task refs resolve to real tasks
- `blocked_by` external refs are format-validated
- `sprint` field references an existing sprint
- Sprint index entries resolve to real tasks
- No circular blocker dependencies
- Task ID matches filename prefix
- No duplicate IDs

Returns exit 0 on a clean graph, exit 1 with all errors listed.

## Architecture

Two crates:

- `stint-core` — pure library, zero I/O. Schema types, parsing, validation, mutation.
- `stint-cli` — thin Clap shell. All business logic lives in core.

The separation means `stint-core` can be embedded in other tools (a Plexi TUI app, an editor plugin, an agent) without pulling in CLI concerns.

## Roadmap

- [ ] `stint gh import` — pull GitHub issues as tasks
- [ ] `stint gh sync` — push task status back to GitHub
- [ ] `stint check --cross-repo` — resolve cross-repo blockers
- [ ] Plexi app — TUI visualization backed by `stint-core`
