---
id: "0004"
title: "CLI argument parsing"
status: todo
priority: p2
size: s
estimate: "1.5h"
sprint: "s1"
blocked_by:
  - "0001"
area:
  - "cli"
tags:
  - "ux"
---

Parse `weather <city> [--units metric|imperial]` with clap.

## Why

Depends on the crate scaffold (0001). Feeds the render task (0005) the city
and units to display.

## Done When

- [ ] CLI flags for city name and `--units` parsed into arguments struct
- [ ] Invalid units trigger user-friendly error message with accepted options
- [ ] `--help` displays clear documentation for each flag
