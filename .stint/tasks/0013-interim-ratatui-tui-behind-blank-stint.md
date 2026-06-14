---
id: "0013"
title: "Interim ratatui TUI behind blank stint"
status: done
estimate: "16h"
completed_at: "2026-06-14T17:31:51Z"
blocked_by: []
gh_issue: []
area:
  - "cli"
tags:
  - "tui"
  - "ux"
---


## Why

The CLI is now legible (`stint next` shows blocked reasons, `stint list` shows
a computed STATE column, both colored), but organizing, creating, moving, and
re-prioritizing tasks across a sprint is still a multi-command text dance. A
read-and-act TUI gives an at-a-glance board plus single-keystroke mutation,
without waiting on the eventual Plexi-native app ([[0004-tui-visualization]],
which is blocked on the Plexi interface and is the long-term home for this).

This task is the **interim** ratatui TUI that ships inside the `stint` binary
and drives the existing core library directly. It is throwaway-tolerant: when
the Plexi PGAP app lands, this can be retired. It must not add business logic —
all state lives in `stint-core`; the TUI is a pure view + command dispatcher.

## Goal

Running `stint` with no subcommand launches a full-screen terminal app that
renders the current sprint as state-grouped columns and lets the user perform
the common mutations the CLI already supports, reading and writing the same
`.stint/` markdown files live.

## Scope (v1)

Columns by `TaskState` (the canonical `state::classify` output): Ready,
Active, Blocked, Backlog, Done. Each card shows id, title, estimate, and — for
Blocked — the reason (`blocked by NNNN` / `area busy: NNNN` / `area taken this
run by NNNN`), reusing the exact strings the CLI prints so the two surfaces
never disagree.

Navigation:
- arrow / `hjkl` to move between cards and columns
- `enter` opens the task body in `$EDITOR` (suspend TUI, resume on exit)
- `o` reveals the task file path / `gf`-style jump for editor users

Mutations (all via existing core functions, written through the same file I/O
the CLI uses — no new write path):
- `s` start (todo → in-progress), `d` done, `a` archive
- `r` promote backlog → todo (`ready`), `b` defer todo → backlog
- `n` new task (title prompt → `add`)
- `[` / `]` move selected task earlier/later in the sprint order
- `c` run `check` and surface errors in a footer banner

Live refresh: a `notify` watcher (std thread → `mpsc`, no async runtime) re-reads
`.stint/` on external change, debounced ~150ms and ignoring the TUI's own writes,
so a background agent claiming a task or editing a file reflects without a
keystroke. Also re-read after every mutation and after returning from `$EDITOR`.

No live timer. The workspace is agent-driven, not human-clocked; time stays
`actual` accrued via `stint log` (see `docs/TUI_DESIGN.md` §"Time tracking").

Empty/edge states: no `.stint/` workspace → offer `init`; no sprint → prompt to
create one; check failures shown non-blocking in a status bar.

## Out of scope (v1)

- Editing frontmatter fields inline (estimate, area, tags) — use `$EDITOR`.
- Cross-repo / external blocker resolution views.
- Mouse support and themes (both 0004). Honor `NO_COLOR` and degrade to plain.
- The `Graph` DAG view (0004 — needs the Plexi render layer).
- Anything that duplicates business logic out of `stint-core`.

Note: `.stint/config.toml` *is* read in this build — custom commands (suspend-and-
run) are defined there. Only the inline TUI-config/theming surface is out of scope.

## Architecture

- New `src/tui/` module in the binary crate only. Zero additions to the
  library's public surface beyond what `cmds`/core already expose.
- `stint` with no args dispatches to `tui::run()`; every keypress maps to an
  existing `cmds::cmd_*` call so behavior is identical to the CLI.
- The board model is derived each frame from `repo.load_tasks()` +
  `compute_next` + `classify`; the TUI holds no authoritative state.
- Crates: `ratatui` + `crossterm` + `notify`. **No `tokio`** — the render loop
  uses `crossterm::event::poll(timeout)`, the watcher runs on a std thread feeding
  an `mpsc` channel, and custom commands run via suspended `std::process::Command`.
  Async/panes are 0004's concern, not this interim build's.
- Gate launch on an interactive terminal; if stdout is not a tty, print the current
  `status` summary and exit (so `stint` in a pipe stays scriptable).

## Acceptance

- `stint` (no args) in a workspace opens the board; `q` / `Ctrl-C` restores the
  terminal cleanly (raw mode + alternate screen always torn down on panic).
- Every mutation listed above changes the underlying markdown identically to
  the equivalent CLI command (verified by diffing files before/after).
- Blocked-card reasons are byte-identical to `stint next` output.
- `stint | cat` (non-tty) prints the status summary, does not launch the TUI.
- No new logic in `stint-core`; `cargo test` still green.

## Gotchas

- Always tear down raw mode / alternate screen on panic — install a hook, or
  the user's terminal is left wedged.
- Suspending for `$EDITOR` must leave and re-enter the alternate screen
  correctly; test with both `nvim` and `vi`.
- Re-read from disk after `$EDITOR` and after mutations; never trust an
  in-memory cache as source of truth — `.stint/` files are the truth.
- This is explicitly interim. Do not let TUI-only behavior leak into core; it
  must remain retire-able when [[0004-tui-visualization]] (Plexi-native) ships.

## References

- `docs/TUI_DESIGN.md` — the full best-case design this task delivers the
  interim path toward (incl. framework rationale).
- [[0004-tui-visualization]] — long-term Plexi PGAP version of the board.
- `NORTH_STAR.md`, `docs/PRD.md` for vision and schema.
- ratatui — https://ratatui.rs · https://docs.rs/ratatui · examples:
  https://github.com/ratatui/ratatui/tree/main/examples
- crossterm (backend) — https://docs.rs/crossterm
- notify (file watcher) — https://docs.rs/notify
