---
id: "0009"
title: "Auto-detect location via Geo-IP when no city argument is supplied"
status: proposed
area:
  - "network"
tags:
  - "ux"
---

When running `weather` without arguments or config, query a lightweight Geo-IP lookup service to resolve the user's approximate city.

## Why

Improves the out-of-the-box onboarding experience by giving an immediate local forecast without requiring upfront config.

## Done When

- [ ] Query lightweight IP lookup service when no city argument is given
- [ ] Timeout in under 500ms with fallback to default city if offline
- [ ] Privacy flag `--no-ip-lookup` allows opting out
