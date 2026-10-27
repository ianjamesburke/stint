# stint

Git-tracked task management, dependency DAGs, and decision gating as markdown files in your repo. An alternative to issue trackers for teams and autonomous agents who want their task pipeline versioned with their code.

Tasks live in `.stint/tasks/`.

`.stint/` commits alongside your code, or gitignore it to keep tasks private, or initialize it as a separate git repo inside the same directory for clean separation.

```
.stint/
  tasks/
    0001-auth-middleware.md
    0002-tui-skeleton.md
    0003-gate-retry-policy.md
  config.toml
```

## Install

```bash
cargo install stint --locked
```

For zsh tab completion (cargo install users only — the shell installer handles this automatically):

```bash
mkdir -p ~/.zsh/completions
stint completions zsh > ~/.zsh/completions/_stint
# ensure ~/.zshrc has: fpath=(~/.zsh/completions $fpath) && autoload -U compinit && compinit
```

To update:

```bash
stint update
# same as: cargo install stint --locked --force
```

## Release

Release notes are generated with `git-cliff`.

```bash
just bump          # patch bump, changelog, release commit
just publish-check # crates.io dry run
git push origin main --follow-tags
```

There is no publish workflow; crates.io is updated by hand with `cargo publish`
after `just publish-check`. The tag is annotated so `--follow-tags` pushes it.

## Usage

```bash
# Tasks
stint add "Implement auth middleware" --priority p1 --size m  # lands in todo, schedulable by `stint next`
stint add "Someday: dark mode" --backlog                       # icebox instead; promote later with `stint ready`
stint propose "Explore WebSocket sync" --why "Polling is slow" # candidate PRD outside active queue
stint approve 0003 --priority p2 --size s --done-when "Bench <50ms" # promote proposal to todo
stint reject 0003 --reason "Out of scope for v1"             # archive proposal

# Querying & Scheduling
stint list
stint list --status in-progress
stint list --priority p0
stint list --blocked
stint list --hide-blocked
stint show 1
stint next
stint claim                                                  # auto-claim top ready task under an advisory lock
stint claim 1                                                # claim specific task
stint unclaim 1                                              # revert in-progress task to todo
stint log 1 2h
stint done 1 --actual 3h
stint archive 1

# Human Decision Gates
stint gate file 1 "Choose retry strategy" --ask "Should we retry 5xx synchronously or enqueue to DLQ?" \
    --options "(recommended) Option A: DLQ" --options "Option B: Sync retry" \
    --good-answer "Pick A or B and max retry count" --test "keep latency < 200ms" \
    --evidence "Stripe times out after 10s" --links "src/webhook.rs"
stint gate list
stint gate show 2
stint gate resolve 2 --decision "Option A (DLQ) with 3 retries"

# Graph Validation & Status
stint check
stint check --strict                                         # enforce pass criteria on active tasks
stint status
stint guide                                                  # built-in operator guide
stint schema                                                 # machine-readable JSON schema
```

## Next work

`stint next` derives claimable work from the task dependency graph.

- tasks must be `todo` (or `backlog` with `--include-backlog`)
- local task blockers must be `done`
- all other blocker types (issues, external tasks, notes) make a task blocked
- `priority` (P0 highest, P4 lowest) sorts candidates; creation timestamp and ID break ties
- tasks without a priority sort after all prioritized tasks
- tasks whose `area` overlaps with `in-progress` work are hidden by default
- `stint claim` claims the top unblocked, non-conflicting task under a lock

Use `stint next --include-area-conflicts` to see ready tasks even if they touch
an area already in progress.

## Task format

```markdown
---
id: "0001"
title: Auth middleware
status: in-progress
priority: p1
size: m
estimate: 4h
actual: 2h
blocked_by:
  - 2                          # local task 0002
  - @847                       # local GitHub issue #847
  - acme/sdk@847               # external GitHub issue
  - ianjamesburke/PLEXI:0146   # task in another GitHub repo
  - ../PLEXI:0146              # task in a sibling directory
  - "waiting for design"       # free-text note
gh_issue: [123]
area: [backend]
tags: []
---

## Why

Why this task exists and what problem it solves.

## Done When

- [ ] Every request validates JWT Bearer tokens
- [ ] Integration tests pass in CI

## References

- src/auth.rs
```

`blocked_by` is a unified field — type is inferred by syntax. `gh_issue`, `area`, and `tags` all accept a single value or a list.

| Syntax | Meaning |
|---|---|
| bare integer | local stint task (auto-padded to 4 digits) |
| `@N` | local GitHub issue |
| `owner/repo@N` | external GitHub issue |
| `owner/repo:NNNN` | task in an external GitHub repo |
| `../path:NNNN` | task in a sibling local directory |
| `../path@N` | issue in a sibling local directory |
| quoted string | free-text blocker note |

## Validation

`stint check` validates the entire task graph:

- Required fields present (`id`, `title`, `status`)
- Status is a valid enum value (`proposed`, `backlog`, `todo`, `in-progress`, `done`, `archived`)
- Priority is a valid enum value if present (`p0`, `p1`, `p2`, `p3`, `p4`)
- Size is a valid enum value if present (`s`, `m`, `l`)
- Duration strings are valid (`4h`, `30m`, `1.5h`)
- RFC3339 timestamps for lifecycle fields
- `blocked_by` local task refs resolve to real tasks
- `blocked_by` external refs are format-validated
- No unresolved blockers on `in-progress` or `done` tasks
- No circular blocker dependencies
- Gate tasks contain all 7 required sections
- Task ID matches 4-digit filename prefix
- No duplicate IDs

`stint check --strict` additionally enforces pass criteria (`priority`, `size`, `## Why`, and `## Done When`) on all active pipeline tasks.

Returns exit 0 on a clean graph, exit 1 with all errors listed.

## Agent guide & schema

Stint includes built-in reference guides and machine-readable schemas:

```bash
stint guide [overview|add|propose|implement|gate|check]
stint schema [task|gate|cli] --json
```
