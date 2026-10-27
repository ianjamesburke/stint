---
id: "0007"
title: "Token-bucket rate limiting with Redis"
status: proposed
area:
  - "infra"
tags:
  - "rate-limiting"
---

Protect public endpoints against denial of service and brute force using Redis token-bucket counters.

## Why

Candidate protection mechanism discovered during threat modeling. Kept in `proposed` pending infrastructure capacity review.

## Done When

- [ ] Rate limiting middleware limits unauthenticated requests to 60 req/min per IP
- [ ] Exceeded quota responds with HTTP 429 Too Many Requests and Retry-After header
- [ ] Distributed Redis lua script executes atomic counter increments
