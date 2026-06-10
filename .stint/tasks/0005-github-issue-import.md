---
id: "0005"
title: "GitHub issue import (stint gh import)"
status: backlog
estimate: "3h"
sprint: "s1"
area:
  - "cli"
tags:
  - "gh-integration"
blocked_by: []
blocked_by_gh: []
gh_issue: []
---

## Why

Teams track work in GitHub Issues. `stint gh import <N>` should pull issue
title, body, and labels into a new task file so the two systems stay in sync
without manual copying.

## Gotchas

Requires `gh` CLI to be available and authenticated. Infer repo from git remote
if `.stint/config.toml` has no `[gh] repo` entry.
