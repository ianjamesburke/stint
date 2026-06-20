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
status: backlog                   # required, enum: backlog|todo|in-progress|done|archived
priority: p2                      # optional, enum: p0|p1|p2|p3|p4 (p0 = highest)
estimate: "4h"                    # optional, string (e.g. "2h", "30m", "1.5h")
actual: "0h"                      # optional, string, time logged so far
created_at: "2026-06-14T12:00:00Z" # optional, RFC3339 UTC timestamp
sprint: "s12"                     # optional, string, sprint ID this task belongs to
blocked_by: []                    # optional, unified blocker list (see Blocker Model)
gh_issue: []                      # optional, list OR single int/string of GH issue numbers
area: []                          # optional, list OR string of area labels
tags: []                          # optional, list OR string of arbitrary tags
---
```

**Coercion rule:** `blocked_by`, `gh_issue`, `area`, `tags` accept either a single scalar value or a list. The library normalizes all of these internally at parse time. `stint check` validates types after coercion.

### Body

Everything below `---` is freeform markdown. No structure enforced. Typical sections:

```markdown
## Why

Why this task exists and what problem it solves.

## Gotchas

Non-obvious constraints, prior attempts, things that will bite you.

## References

- `src/auth/middleware.rs:142` — relevant code location
- owner/repo#847 — upstream dependency
```

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

### Task management

| Command | Description |
|---|---|
| `stint add "Title"` | Create a new task, open editor for body |
| `stint list` | List all tasks (filterable by status, sprint, area, tag) |
| `stint list --sprint s12` | List tasks in a sprint |
| `stint list --status in-progress` | Filter by status |
| `stint show <id>` | Print full task (frontmatter + body) |
| `stint edit <id>` | Open task file in $EDITOR |
| `stint done <id>` | Mark task done, prompt for actual time |
| `stint log <id> <time>` | Add time to actual (e.g. `stint log 0001 2h`) |
| `stint archive <id>` | Move task to archived status |

### Sprint management

| Command | Description |
|---|---|
| `stint sprint new <id> <date-range>` | Create a new sprint index file |
| `stint sprint list` | List all sprints |
| `stint sprint show <id>` | Show sprint with task summaries |
| `stint sprint add <sprint-id> <task-id>` | Append task to sprint |
| `stint sprint remove <sprint-id> <task-id>` | Remove task from sprint |
| `stint sprint reorder <id>` | Interactive reorder (uses $EDITOR) |

### Validation

| Command | Description |
|---|---|
| `stint check` | Validate entire task graph |
| `stint check --cross-repo` | Also resolve cross-repo blocked_by refs |
| `stint status` | Summary: open tasks, blocked tasks, sprint progress |

### GitHub integration (optional)

| Command | Description |
|---|---|
| `stint gh import <issue-number>` | Import a GH issue as a task |
| `stint gh import --all --label "sprint-12"` | Batch import by label |
| `stint gh sync <id>` | Push task status back to GH issue |

---

## `stint check` Validation Rules

1. All required fields present (`id`, `title`, `status`)
2. `status` is a valid enum value
3. `estimate` and `actual` are valid duration strings if present
4. `blocked_by` local task refs resolve to existing task files
5. `blocked_by` external refs are structurally valid (format check only)
6. `sprint` field references an existing sprint file if present
7. Sprint index files reference only existing task IDs
8. No circular `blocked_by` references
9. `id` matches the numeric prefix of the filename
10. No duplicate IDs across all task files
11. No `in-progress`/`done` task has an unresolved local-task blocker (only `backlog`/`todo` tasks may carry active blockers)
12. Timestamp fields (`created_at`, `started_at`, `completed_at`) are valid RFC3339 if present
13. A task listed in a sprint's index file must have a matching `sprint` frontmatter field (bidirectional consistency)
14. A task with a `sprint` frontmatter field must be listed in that sprint's index file (bidirectional consistency)

---

## Blocker Model

`blocked_by` is a single unified field. Type is inferred by syntax at parse time:

| Syntax | Meaning | Validated |
|---|---|---|
| bare integer or all-digit string | local stint task (zero-padded) | yes — must resolve to existing task |
| `@N` | local GitHub issue | no |
| `owner/repo@N` | external GitHub issue | format only |
| `owner/repo:NNNN` | task in external GitHub repo | format only |
| `../path:NNNN` | task in sibling local directory | format only |
| `../path@N` | issue in sibling local directory | format only |
| quoted string | free-text note | no |

`stint status` renders all active blocker types in a unified list; done
local-task blockers are ignored. `stint check --cross-repo` walks sibling repos
with `.stint/` directories to resolve local-dir and external task refs.

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
