---
id: "0003"
title: "Add stint update command"
status: done
estimate: "2h"
actual: "2h"
completed_at: "2026-06-10T20:45:00Z"
blocked_by: []
gh_issue: []
area:
  - "cli"
tags: []
---


## Why

`stint edit` always opens `$EDITOR`, which is slow for scripted or quick field
changes. `stint update` lets callers set individual fields (status, estimate,
title, sprint, blocked_by_note) in one command with no interactive step.

## Gotchas

Vec fields (area, tags, blocked_by) are intentionally excluded from update — 
the semantics of append vs replace are unclear without more use cases. Use
`stint edit` for those.
