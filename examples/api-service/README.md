# Demo: `api-service`

A multi-area API service workspace demonstrating:
- **Parallel area dispatch**: Tasks in distinct areas (`auth`, `storage`, `telemetry`) run concurrently without collisions.
- **Architectural decision gates**: `stint gate` tracks blocking security and parameter decisions before implementation proceeds.
- **Multi-sprint planning**: Separate sprint milestones (`s1.md` for core foundation, `s2.md` for billing).
- **Proposals (`proposed`)**: Draft ideas filed outside the active pipeline pending team review.

## The Dependency & Area Graph

```
[Area: auth]
0001 jwt middleware (in-progress) ──┬──> 0003 user registration (blocked)
                                    │      ▲
[Area: storage]                     │      │
0002 db connection pool (ready) ────┤      │
                                    │      │
[Area: telemetry]                   │      │
0004 prometheus metrics (ready)     │      │
                                    │      │
[Decision Gate]                     │      │
0005 argon2id parameters ───────────┴──────┘

[Sprint 2 / Backlog]
0006 stripe webhook (s2)

[Proposed Ideas]
0007 redis rate limiting
```

| Task | Area | Status | Priority | Size | State in `stint next` |
|------|------|--------|----------|------|-----------------------|
| `0001` | `auth` | `in-progress` | `p0` | `m` | Active |
| `0002` | `storage` | `todo` | `p0` | `m` | **Ready** (no area conflict with 0001) |
| `0004` | `telemetry`| `todo` | `p2` | `s` | **Ready** (parallel unblocked track) |
| `0003` | `auth` | `todo` | `p1` | `m` | Blocked by 0001, 0002, 0005 |
| `0005` | `auth` | `backlog` (`gate`)| — | — | Gate (blocks 0003) |
| `0006` | `billing`| `backlog` | `p2` | `l` | Icebox (Sprint 2) |
| `0007` | `infra` | `proposed` | — | — | Awaiting review (`stint approve`) |

---

## Commands to Try

```bash
cd examples/api-service

# 1. Inspect status and parallel ready tasks
stint status
stint next

# 2. Claim the storage task without conflicting with in-progress auth work
stint claim 0002

# 3. Inspect and resolve the password hashing gate
stint gate show 0005
stint gate resolve 0005 --decision "Argon2id (m=19456, t=2, p=1)"

# 4. Check sprint status
stint sprint list
stint sprint show s1

# 5. Strict schema validation
stint check --strict
```
