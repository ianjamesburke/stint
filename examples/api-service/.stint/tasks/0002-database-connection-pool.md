---
id: "0002"
title: "Setup connection pool and migration runner"
status: todo
priority: p0
size: m
estimate: "2.5h"
sprint: "s1"
area:
  - "storage"
tags:
  - "database"
---

Initialize asynchronous PostgreSQL connection pool and embed automated database migrations.

## Why

Foundation for persistent user records and account state. Lives in `storage` area, allowing it to proceed in parallel with `auth` work.

## Done When

- [ ] Connection pool connects using DATABASE_URL environment variable
- [ ] Database migrations run automatically on service startup
- [ ] Health ping verifies database connectivity
