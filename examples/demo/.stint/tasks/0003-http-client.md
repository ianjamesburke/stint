---
id: "0003"
title: "HTTP client for the weather API"
status: todo
estimate: "3h"
sprint: "s1"
blocked_by:
  - 0001
area:
  - "network"
tags:
  - "api"
---

Fetch the 3-day forecast JSON from the upstream weather API and deserialize it.

## Why

Depends on the crate scaffold (0001). Together with arg parsing (0004) it
unblocks the final render task (0005).
