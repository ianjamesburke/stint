---
id: "0006"
title: "Read .stint/config.toml"
status: todo
estimate: "2h"
area:
  - "core"
  - "cli"
blocked_by: []
blocked_by_gh: []
gh_issue: []
---

## Why

`default_sprint` and `[gh] repo` need a home. Without config.toml, every
command that needs them falls back to heuristics or requires explicit flags.

## Gotchas

Missing required fields must throw, not silently fall back. Optional fields
must be clearly marked as such in the schema.
