---
id: "0001"
title: "Project scaffold"
status: todo
estimate: "1h"
sprint: "s1"
area:
  - "build"
tags:
  - "setup"
---

Create the cargo project, module layout, and a no-op `weather` binary that
parses `--help`. Everything else in the sprint builds on this.

## Why

This is the foundation task. Config loading, the HTTP client, and arg parsing
all need the crate to exist first, so it blocks the rest of the sprint. It has
no blockers itself, which makes it both **ready** and the **bottleneck**.
