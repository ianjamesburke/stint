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
sprint, each with its area and (if an external process claimed it) what claimed it.
This surface is built for agentic operation: agents claim and complete tasks
directly on disk, and the Dashboard is the live "what is every agent working on"
screen you leave open.

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
agent claims a task or edits a task file directly, the card moves to Active and the
Dashboard updates — no refresh keystroke. The TUI re-derives its model from disk on
every change and after every `$EDITOR` return. This is a hard requirement, not a
nicety: this workspace is driven by agents concurrently with the human watching.

The watcher must not loop on the TUI's own writes. Coalesce events with a short
debounce (~150ms) and ignore events for paths the TUI just wrote (track an
in-flight write set / compare the mtime it just stamped). Without this, every
mutation triggers a self-reload, jumping the cursor and flickering the frame.

## Inline editing & blockers

- Edit any frontmatter field inline: title, estimate, area, tags, sprint, status,
  and `blocked_by` — the last with fuzzy autocomplete over existing task ids and
  cross-repo `../repo:id` refs.
- See blockers both directions: what blocks a task *and* what it blocks.
  Add/remove edges live; resolving one shows the cascade of newly-ready tasks.
- Continuous `check`: validation runs in the background and surfaces errors inline
  on the offending card with a one-key jump-to-fix.

## Time tracking

No live timer. This workspace is agent-driven, not human-clocked, so there is no
`space`-to-start stopwatch and no `started_at` schema field. Time is recorded the
way the CLI already does it: `actual` accrues via `stint log <id> <duration>`
(PRD §Time Tracking), and the Sprint burndown reads `estimate` vs `actual`. The
TUI surfaces those values; it does not run a clock.

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

Execution model: when running inside Plexi (0004), a command **opens a real pane**
and launches there (e.g. `plexi run claude` spawns a pane running Claude scoped to
the task). Standalone — including the interim 0013 build — the TUI **suspends,
runs the command attached, and resumes on exit** (leave/re-enter the alternate
screen cleanly). Terminal-multiplexer and Plexi pane-splitting are 0004's job, not
0013's; do not detect/branch on `$TMUX`/`$ZELLIJ` in the interim build.

Ordering and failure semantics: if `claim = true`, the claim write (status →
in-progress) happens **first**, then the command spawns. Changing status does not
rename the task file, so `{path}` stays valid across the claim. A non-zero exit
does **not** auto-unclaim — a launch is a launch; unclaim explicitly if needed.

## Architecture

- The view layer sits over `stint-core`, which stays pure. Add a binary-side
  **command + journal layer**: every user action is a typed Command that maps to
  one or more core mutations plus file writes, recorded for undo/redo. Core gains
  no UI logic.
- Undo/redo replays **inverse commands** from an in-session journal scoped to the
  fields the action touched. It is *not* `git revert`/`reset`: in a workspace
  agents write concurrently, a git-level undo would clobber their in-flight work.
  Git stays the audit log; the journal is the undo engine.
- File watcher (`notify`) drives live refresh; disk is authoritative. Debounce and
  ignore self-writes (see "Live, agent-tolerant state").
- Concurrency: the interim 0013 build needs **no async runtime** —
  `crossterm::event::poll(timeout)` in the render loop, plus a `notify` watcher on
  a std thread feeding an `mpsc` channel, and `std::process::Command` (suspended)
  for custom commands, covers it. `tokio` belongs to 0004, where the Plexi runtime
  supplies async, panes, and git orchestration.
- Stack: `ratatui` + `crossterm` + `notify` (watcher in both). Mouse support and
  themes are 0004-only; no themes anywhere. Non-tty → scriptable summary and exit,
  so `stint` in a pipe stays automatable.

## What ships where (0013 interim vs 0004 Plexi-native)

| Feature | 0013 interim | 0004 Plexi-native |
|---|---|---|
| Board / Table / Sprint / Dashboard views | ✅ | ✅ |
| Graph (DAG / critical path / bottleneck cone) | ❌ deferred | ✅ (Plexi render) |
| Blocked-reason strings match `stint next` | ✅ | ✅ |
| File watcher, agent-tolerant refresh | ✅ | ✅ |
| Inline frontmatter edit | ❌ `$EDITOR` only | ✅ |
| Custom commands (`.stint/config.toml`) | ✅ suspend-and-run | ✅ real panes |
| Undo/redo journal | ✅ basic | ✅ |
| Search `/` · sort `s` · filter `f` | ✅ | ✅ |
| Mouse support | ❌ | ✅ |
| Async (`tokio`), pane management | ❌ | ✅ (runtime) |

`/`, `s`, and `f` state is per-view and resets on `tab` to keep the model simple;
a persisted global filter is a later refinement, not a v1 requirement.

## Out of scope

- Themes / configurable color schemes.
- GitHub issue import from the UI (stays a CLI concern: `stint init --with-github`).
