---
id: "0004"
title: "TUI sprint board visualization"
status: backlog
estimate: "12h"
area:
  - "apps"
tags:
  - "tui"
  - "plexi"
blocked_by:
  - "0003"
blocked_by_gh: []
gh_issue: []
---

## Why

`stint status` and `stint list` are text-only. A TUI board (Plexi PGAP app)
would give a live kanban view of the current sprint with task details on
selection.

## Gotchas

Plexi PGAP apps require a valid `manifest.toml` — always scaffold with
`plexi app init`, never write by hand.
