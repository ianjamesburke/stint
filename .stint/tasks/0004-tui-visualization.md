---
id: "0004"
title: "TUI sprint board visualization"
status: archived
estimate: "12h"
blocked_by:
  - 3
gh_issue: []
area:
  - "apps"
tags:
  - "tui"
  - "plexi"
---


## Why

`stint status` and `stint list` are text-only. A TUI board (Plexi PGAP app)
would give a live kanban view of the current sprint with task details on
selection.

## Gotchas

Plexi PGAP apps require a valid `manifest.toml` — always scaffold with
`plexi app init`, never write by hand.

## References

- `docs/TUI_DESIGN.md` — the full best-case design; this task is its
  long-term Plexi-native home (graph, async, pane management come from the
  Plexi runtime). [[0013-interim-ratatui-tui-behind-blank-stint]] is the
  interim path.
