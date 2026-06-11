---
id: "0002"
title: "Implement stint check validation"
status: done
estimate: "4h"
actual: "3.5h"
area:
  - "core"
tags:
  - "validation"
blocked_by:
  - "0001"
gh_issue: []
---

## Why

`stint check` is the source of truth for schema correctness. All 10 validation
rules must be enforced before any other tooling can trust the task graph.

## Gotchas

Circular blocked_by detection requires a DFS traversal — a simple forward pass
will miss cycles of length > 2.
