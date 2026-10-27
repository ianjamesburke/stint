---
id: "0003"
title: "HTTP client for the weather API"
status: todo
priority: p1
size: m
estimate: "3h"
sprint: "s1"
blocked_by:
  - "0001"
area:
  - "network"
tags:
  - "api"
---

Fetch the 3-day forecast JSON from the upstream weather API and deserialize it.

## Why

Depends on the crate scaffold (0001). Together with arg parsing (0004) it
unblocks the final render task (0005).

## Done When

- [ ] HTTP client sends GET request to upstream endpoint with query parameters
- [ ] Forecast JSON deserializes into strongly-typed weather data structs
- [ ] Network timeout and retry logic handles transient errors gracefully
- [ ] Integration tests verify payload parsing against mock server
