---
id: "0005"
title: "Render the forecast to the terminal"
status: todo
estimate: "2h"
sprint: "s1"
blocked_by:
  - 0003
  - 0004
area:
  - "cli"
tags:
  - "ux"
---

Format the fetched forecast into a readable 3-day table.

## Why

The payoff task. Needs both the HTTP client (0003) for data and arg parsing
(0004) for the city/units. Blocked by two tasks, so it surfaces last.
