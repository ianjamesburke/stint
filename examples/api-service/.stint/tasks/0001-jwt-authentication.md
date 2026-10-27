---
id: "0001"
title: "Implement JWT authentication middleware"
status: in-progress
priority: p0
size: m
estimate: "2h"
started_at: "2026-10-27T14:00:00Z"
sprint: "s1"
area:
  - "auth"
tags:
  - "security"
---

Validate incoming Authorization header bearer tokens against signing secret.

## Why

All protected API endpoints require authenticating user claims before processing requests.

## Done When

- [ ] Middleware extracts Bearer token from Authorization header
- [ ] Valid tokens populate RequestContext with user identity and scopes
- [ ] Expired or invalid signatures return 401 Unauthorized with descriptive JSON error
- [ ] Integration tests verify valid and expired token flows
