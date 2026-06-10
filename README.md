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
- local `blocked_by` tasks must be `done`
- `blocked_by_gh` and `blocked_by_note` make a task blocked
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
blocked_by: []
blocked_by_gh: [anthropics/sdk#847]
blocked_by_note: ""
gh_issue: [123]
area: [backend]
tags: []
---

## Why

Why this task exists and what problem it solves.

## Gotchas

Non-obvious constraints, prior attempts, things that will bite you.
```

`blocked_by`, `blocked_by_gh`, `gh_issue`, `area`, and `tags` all accept a single value or a list.

## Sprint format

```markdown
# Sprint 1 · Jun 9–20 · goal: ship v1

- 0001-auth-middleware
- 0002-tui-skeleton
```

Line order is priority order. Edit directly or use `stint sprint reorder`.

## Validation

`stint check` validates the entire task graph:

- Required fields present
- Status is a valid enum value
- Duration strings are valid (`4h`, `30m`, `1.5h`)
- `blocked_by` IDs resolve to real tasks
- `blocked_by_gh` entries match `owner/repo#N` format
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
