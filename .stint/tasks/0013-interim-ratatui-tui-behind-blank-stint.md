---
id: "0013"
title: "Interim ratatui TUI behind blank stint"
status: backlog
estimate: "16h"
area:
  - "cli"
tags:
  - "tui"
  - "ux"
blocked_by: []
gh_issue: []
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

Live refresh: re-read `.stint/` after every mutation and after returning from
`$EDITOR`; reflect external edits on focus regain.

Empty/edge states: no `.stint/` workspace → offer `init`; no sprint → prompt to
create one; check failures shown non-blocking in a status bar.

## Out of scope (v1)

- Editing frontmatter fields inline (estimate, area, tags) — use `$EDITOR`.
- Cross-repo / external blocker resolution views.
- Mouse support, themes, config. Honor `NO_COLOR` and degrade to plain.
- Anything that duplicates business logic out of `stint-core`.

## Architecture

- New `src/tui/` module in the binary crate only. Zero additions to the
  library's public surface beyond what `cmds`/core already expose.
- `stint` with no args dispatches to `tui::run()`; every keypress maps to an
  existing `cmds::cmd_*` call so behavior is identical to the CLI.
- The board model is derived each frame from `repo.load_tasks()` +
  `compute_next` + `classify`; the TUI holds no authoritative state.
- Crates: `ratatui` + `crossterm`. Gate launch on an interactive terminal;
  if stdout is not a tty, print the current `status` summary and exit (so
  `stint` in a pipe stays scriptable).

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
  interim path toward.
- [[0004-tui-visualization]] — long-term Plexi PGAP version of the board.
- `NORTH_STAR.md`, `docs/PRD.md` for vision and schema.
