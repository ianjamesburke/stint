---
id: "0005"
title: "Render the forecast to the terminal"
status: todo
priority: p1
size: m
estimate: "2h"
sprint: "s1"
blocked_by:
  - "0003"
  - "0004"
  - "0008"
area:
  - "cli"
tags:
  - "ux"
---

Format the fetched forecast into a readable 3-day table.

## Why

The payoff task. Needs both the HTTP client (0003) for data, arg parsing
(0004) for city/units, and the output styling gate (0008) resolved to decide
terminal glyph conventions.

## Done When

- [ ] Forecast outputs 3-day overview table with low, high, and conditions
- [ ] Terminal width and colors adapt to user's shell capabilities
- [ ] Snapshot test verifies expected visual layout
