# Demo: `weather-cli`

A tiny, self-contained stint workspace you can poke at to see how tasks,
sprints, and blocking relationships fit together. It plans an imaginary
`weather` CLI across one sprint.

Run any stint command from inside this directory (stint walks up to find the
nearest `.stint/`):

```bash
cd examples/demo
stint status     # sprint progress + what's blocked
stint next       # what to work on, what's waiting, and the bottleneck
stint check      # validate the whole task graph
stint show 0001  # full task detail
```

## The graph

```
0001 scaffold ──┬─> 0002 config loader
                ├─> 0003 http client ──┬─> 0005 render forecast
                ├─> 0004 cli args ─────┘
                └─> 0006 caching (backlog / iced)
0007 readme (done)
```

| Task | Status   | Blocked by | State in `stint next`        |
|------|----------|------------|------------------------------|
| 0001 | todo     | —          | **Ready** + **Bottleneck**   |
| 0002 | todo     | 0001       | Blocked                      |
| 0003 | todo     | 0001       | Blocked                      |
| 0004 | todo     | 0001       | Blocked                      |
| 0005 | todo     | 0003, 0004 | Blocked                      |
| 0006 | backlog  | 0003       | Iced (excluded entirely)     |
| 0007 | done     | —          | —                            |

## What it demonstrates

- **Ready vs Blocked** — `stint next` lists `0001` as the only ready task and
  prints everything waiting underneath it.
- **Bottleneck = the thing to do next.** `0001` is simultaneously *ready* and
  the *bottleneck* (it blocks three tasks). The two always agree now: a
  bottleneck is a task you can actually start, never one already in progress.
- **Backlog is a true icebox.** `0006` never appears in `stint next` until you
  promote it with `stint ready 0006`.
- **The state machine is enforced.** Try starting a blocked task:

  ```bash
  stint start 0002      # 0002 is blocked by the unfinished 0001
  stint check           # ERROR: 0002 is in-progress with an unresolved blocker
  ```

  `stint check` fails (exit 1) because only `backlog`/`todo` tasks may carry
  active blockers. Set `0002` back to `todo` to make the graph valid again.

## Try the happy path

```bash
stint start 0001 && stint done 0001 --actual 1h
stint next        # 0002, 0003, 0004 are now Ready; 0005 still Blocked
```
