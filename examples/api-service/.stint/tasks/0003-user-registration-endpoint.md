---
id: "0003"
title: "Add user registration and password hashing endpoint"
status: todo
priority: p1
size: m
estimate: "3h"
sprint: "s1"
blocked_by:
  - "0001"
  - "0002"
  - "0005"
area:
  - "auth"
tags:
  - "endpoint"
---

Endpoint `POST /v1/auth/register` to create accounts with Argon2id password hashing.

## Why

Requires auth middleware (0001), storage pool (0002), and the password hashing algorithm gate (0005) to be resolved before shipping.

## Done When

- [ ] Validates email format and minimum password complexity (>= 10 chars)
- [ ] Hashes passwords with Argon2id before writing to users table
- [ ] Returns 201 Created with JSON body containing user ID and JWT token
- [ ] Duplicate email registration returns 409 Conflict
