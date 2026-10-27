---
id: "0004"
title: "Prometheus metrics and liveness health check"
status: todo
priority: p2
size: s
estimate: "1h"
sprint: "s1"
area:
  - "telemetry"
tags:
  - "monitoring"
---

Expose `/healthz` for Kubernetes probes and `/metrics` for Prometheus scraping.

## Why

Operational visibility. Unblocked and in its own `telemetry` area, ready for immediate parallel implementation.

## Done When

- [ ] `GET /healthz` returns 200 OK with process uptime
- [ ] `GET /metrics` outputs standard Prometheus formatted request counters and latencies
- [ ] Metrics middleware records HTTP response status codes and elapsed time
