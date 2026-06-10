---
id: "0003"
title: "Add stint update command"
status: in-progress
estimate: "2h"
sprint: "s1"
blocked_by: []
blocked_by_gh: []
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
