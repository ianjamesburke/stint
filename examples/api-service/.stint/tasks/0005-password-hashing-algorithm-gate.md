---
id: "0005"
title: "Decide on password hashing cost parameters for registration"
status: backlog
tags:
  - "gate"
area:
  - "auth"
---

## Ask

Which Argon2id memory and time cost parameters should be configured for user password hashing?

## Options

1. (Recommended) Argon2id with m=19456 (19 MiB), t=2 iterations, p=1 parallel lane (~50ms hashing latency on modern cloud vCPUs).
2. OWASP high-security recommendation: m=65536 (64 MiB), t=3 iterations (~250ms latency, high memory pressure on small API containers).
3. Bcrypt with cost factor 12.

## Good answer

Selects option 1 or 2, verifying maximum allowable memory budget per concurrent registration on 512MB container instances.

## Test

Run `cargo bench --bench password_hash` on target hardware to measure throughput and memory overhead.

## Evidence

Argon2id (19 MiB, t=2) meets RFC 9106 minimum guidelines for server authentication and resists GPU dictionary attacks while keeping p99 registration latency below 100ms.

## Links

- Blocks: task 0003 (`0003-user-registration-endpoint.md`)
- RFC 9106 Argon2 profile recommendations: https://datatracker.ietf.org/doc/html/rfc9106

## Decision

