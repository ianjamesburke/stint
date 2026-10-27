---
id: "0006"
title: "Cache API responses on disk"
status: backlog
priority: p3
size: m
estimate: "3h"
sprint: "s1"
blocked_by:
  - "0003"
area:
  - "network"
tags:
  - "perf"
---

Cache forecast responses for 10 minutes to avoid hammering the API.

## Why

A nice-to-have, deliberately left in the **backlog** (iced). It depends on the
HTTP client (0003) but is not part of the committed work, so `stint next`
excludes it entirely until it is promoted with `stint approve 0006`.

## Done When

- [ ] File-based cache keying on location and date stored in cache directory
- [ ] Freshness threshold (10 minutes) verified with expiry check
- [ ] Offline mode reads from cache when network request fails
