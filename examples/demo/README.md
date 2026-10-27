# Demo: `weather-cli`

A self-contained stint workspace showcasing the full stint workflow: task dependency graphs, priorities, sizing, proposals, human decision gates, and sprint progress tracking. It plans an imaginary `weather` CLI across one sprint.

`.stint/sprints/s1.md` lists tasks as clickable markdown links (`[0001](../tasks/0001-project-scaffold.md)`), so cmd-clicking in your editor navigates directly to the task.

Run any stint command from inside this directory:

```bash
cd examples/demo
stint status          # high-level dashboard: active, backlog, gates, sprint progress
stint next            # discover the ready unblocked task to work on next
stint gate list       # inspect pending human decision gates
stint check --strict  # validate pass criteria, schemas, and dependency graph
```

## The Dependency Graph

```
0001 scaffold ──┬─> 0002 config loader
                ├─> 0003 http client ──┬─> 0005 render forecast <── 0008 decision gate
                ├─> 0004 cli args ─────┘
                └─> 0006 caching (backlog)

0007 readme (done)
0009 auto-detect location (proposed idea)
```

| Task | Status | Priority | Size | State in `stint next` | Role in Workflow |
|------|--------|----------|------|-----------------------|------------------|
| `0001` | `todo` | `p1` | `s` | **Ready** (Bottleneck) | Scaffolding foundation; blocks 0002, 0003, 0004 |
| `0002` | `todo` | `p2` | `s` | Blocked by 0001 | Config loader |
| `0003` | `todo` | `p1` | `m` | Blocked by 0001 | HTTP client |
| `0004` | `todo` | `p2` | `s` | Blocked by 0001 | CLI argument parser |
| `0005` | `todo` | `p1` | `m` | Blocked by 0003, 0004, 0008 | Terminal output renderer |
| `0006` | `backlog` | `p3` | `m` | Backlog (Icebox) | Disk cache (promotable via `stint approve`) |
| `0007` | `done` | `p2` | `s` | — | Finished README chore |
| `0008` | `backlog` (`gate`) | — | — | Gate (Blocks 0005) | Human decision on Unicode vs ASCII glyphs |
| `0009` | `proposed` | — | — | Draft Idea | Geo-IP lookup idea outside the active pipeline |

---

## Interactive Walkthrough: Try the Core Workflow

### 1. Inspect the Runway

```bash
stint status
stint next
```

`0001` is identified as the single ready task. It is the bottleneck unlocking the rest of the project.

### 2. Claim and Complete the Foundation

```bash
# Claim the task before starting implementation
stint claim 0001

# Mark it done after writing code (records actual time and unblocks downstream tasks)
stint done 0001 45m

# Notice how 0002, 0003, and 0004 are now Ready!
stint next
```

### 3. Resolve the Decision Gate

Notice that `0005` remains blocked even after 0003 and 0004 finish, because it is waiting on a human decision (`0008`):

```bash
# View the open decision gate and its 7 sections
stint gate show 0008

# Resolve the gate with the decision (marks 0008 done and prunes it from 0005)
stint gate resolve 0008 --decision "Adopt Unicode with ASCII fallback"
```

### 4. Review and Promote a Proposal

Candidate ideas filed with `stint propose` don't pollute the ready queue until reviewed:

```bash
# View proposals
stint list --status proposed

# Approve and promote 0009 into the todo pipeline
stint approve 0009 --priority p2 --size s
```

### 5. Validate Everything

```bash
stint check --strict
```
