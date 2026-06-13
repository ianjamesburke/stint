---
id: "0006"
title: "Cache API responses on disk"
status: backlog
estimate: "3h"
sprint: "s1"
blocked_by:
  - 0003
area:
  - "network"
tags:
  - "perf"
---

Cache forecast responses for 10 minutes to avoid hammering the API.

## Why

A nice-to-have, deliberately left in the **backlog** (iced). It depends on the
HTTP client (0003) but is not part of the committed work, so `stint next`
excludes it entirely until it is promoted with `stint ready 0006`.
