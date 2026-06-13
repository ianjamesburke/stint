# Stint TUI — Design

The best-case design for `stint`'s terminal interface: a full sprint-operations
cockpit, not a viewer. This is the target both implementation tasks aim at —
[`0013`](../.stint/tasks/0013-interim-ratatui-tui-behind-blank-stint.md)
(interim, in the `stint` binary) is the fast path; `0004` (Plexi-native PGAP) is
the long-term home that inherits graph rendering, async, and pane management from
the Plexi runtime.

## Core principle

The TUI is the **primary** interface for everything you can do to a sprint:
create, edit, move, prioritize, unblock, track, and run work. The CLI is the
scripting/automation surface; humans live in the TUI. It reads and writes the
same git-tracked `.stint/` markdown — the disk is always the source of truth, the
TUI never holds authoritative state — but it owns rich interaction the CLI can't.

## Views

`tab` / `shift-tab` cycles views. No top-level themes; honor `NO_COLOR` and
degrade to a plain `status` summary when stdout is not a tty.

**Dashboard** — everything in flight, right now. All `Active` tasks across every
sprint, each with its live timer, elapsed time, area, and (if an external process
claimed it) what claimed it. The "what is happening" screen you leave open.

**Board** — kanban columns by `TaskState`: Backlog · Ready · Blocked · Active ·
Done. Cards show id, title, estimate, area chips, and the blocker reason (the
exact strings `stint next` prints, so the surfaces never disagree). Moving a card
between columns performs the state transition, validated.

**Table** — dense power view. Sortable columns, filterable fields, multi-select
for bulk operations. The triage spreadsheet.

**Graph** — the `blocked_by` DAG. Render the dependency graph, highlight the
critical path, spotlight the bottleneck and its downstream cone. Jump to any
node. This is the observability the text output can't give.

**Sprint** — burndown (estimate vs accumulated actual), progress, goal/date
header, per-area load. Manage sprints: create, edit goal/dates, reorder the task
list, move tasks between sprints.

A **Detail** pane overlays on selection in any view: full frontmatter table plus
rendered markdown body, with inline body editing or `$EDITOR` handoff.

## Keymap

| Key | Action |
|-----|--------|
| `tab` / `shift-tab` | cycle views |
| `hjkl` / arrows | navigate cards / columns / rows |
| `enter` | open Detail (and `$EDITOR` for the body) |
| `/` | fuzzy text search |
| `f` | filter (state / area / tag / sprint) |
| `s` | sort |
| `space` | start / stop the live timer on the selected task |
| `c` | claim / start (todo → in-progress) |
| `d` | done · `a` archive · `r` ready (promote) · `b` defer |
| `[` / `]` | reorder selected task within its sprint |
| `n` | quick new task (title only) · `N` full new-task form |
| `x` | run a custom command on the selected task (menu) |
| `u` / `Ctrl-r` | undo / redo |
| `g` | git: view the `.stint/` diff, commit / push from a footer |
| `:` | command palette (every action + custom commands) |
| `q` | quit (terminal always restored, even on panic) |

## Live, agent-tolerant state

A file watcher (`notify`) reflects external changes immediately. If a background
agent runs `stint start 0013` (or edits a task file directly), the card moves to
Active, the timer picks up `started_at`, and the Dashboard updates — no refresh
keystroke. The TUI re-derives its model from disk on every change and after every
`$EDITOR` return. This is a hard requirement, not a nicety: humans and agents
operate on the same workspace concurrently.

## Inline editing & blockers

- Edit any frontmatter field inline: title, estimate, area, tags, sprint, status,
  and `blocked_by` — the last with fuzzy autocomplete over existing task ids and
  cross-repo `../repo:id` refs.
- See blockers both directions: what blocks a task *and* what it blocks.
  Add/remove edges live; resolving one shows the cascade of newly-ready tasks.
- Continuous `check`: validation runs in the background and surfaces errors inline
  on the offending card with a one-key jump-to-fix.

## Live time tracking

`space` toggles a live timer on the selected task. Elapsed time accrues into
`actual` on stop, is visible on the Dashboard and in the Sprint burndown, and
survives a TUI restart (persisted to the task file). A task can be claimed (`c`)
and timed independently, so you can track time on work an agent is doing.

## Custom commands

User-defined commands that run against the selected task — the way you actually
*start work*. Defined in `.stint/config.toml`:

```toml
[[command]]
key = "k"                 # optional quick key under the `x` menu
name = "Claude on task"
run = "plexi run claude --task {id} --file {path}"
claim = true              # optionally mark the task in-progress when launched

[[command]]
name = "Open worktree + editor"
run = "wtp add stint-{id} && $EDITOR {path}"
```

Placeholders expand from the task: `{id}`, `{slug}`, `{path}`, `{title}`,
`{sprint}`, `{estimate}`. Invoke from the `x` menu or the `:` palette.

Execution model: when running inside Plexi, a command **opens a real pane** and
launches there (e.g. `plexi run claude` spawns a pane running Claude scoped to the
task). Standalone, the TUI runs the command in a split via the detected terminal
multiplexer if available, otherwise suspends, runs attached, and resumes on exit.
If `claim = true`, the task is moved to in-progress (and the timer started) as the
command launches, so "run command on a task" and "start work on it" are one
gesture.

## Architecture

- The view layer sits over `stint-core`, which stays pure. Add a binary-side
  **command + journal layer**: every user action is a typed Command that maps to
  one or more core mutations plus file writes, recorded for undo/redo (backed by
  git). Core gains no UI logic.
- File watcher (`notify`) drives live refresh; disk is authoritative.
- Async runtime (`tokio`) so git ops, custom-command launches, and the watcher
  never block render.
- Stack: `ratatui` + `crossterm`, mouse support. No themes. Non-tty → scriptable
  summary and exit, so `stint` in a pipe stays automatable.

## Out of scope

- Themes / configurable color schemes.
- GitHub issue import from the UI (stays a CLI concern: `stint init --with-github`).
