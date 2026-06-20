---
name: stint
description: Use when working in any repo that uses stint for task and sprint tracking. Covers workflow rules, timing, blockers, and gotchas for the stint CLI.
---

# stint

Thin operating notes for the stint CLI. Do not mirror the full CLI help here; run `stint --help` or `stint <command> --help` for exact syntax.

## Non-negotiables

- Every planned unit of work lives in `.stint/`; do not leave roadmap decisions only in chat or GitHub comments.
- GitHub issues are implementation tickets. Link them from stint tasks with `gh_issue`; keep blockers in `blocked_by` (unified polymorphic field — accepts string or list).
- Sprint order matters. Add work to an existing sprint when it fits; create a new sprint only when the work is genuinely a distinct lane.
- Run `stint check` after editing tasks or sprints.

## Timing

- Run `stint start <id>` as the **first step inside the worktree**, never from the base branch before creating it. Running it on the base branch and then creating a worktree causes both branches to write independent `started_at` timestamps, producing a guaranteed rebase conflict.
- Complete implementation with `stint done <id>` so `completed_at` and `actual` are recorded together.
- Use timestamp/actual override flags only for backfills or corrections; check subcommand help for exact flags.
- If work blocks or is abandoned, do not mark done. Leave the start time in place and document the blocker in the task or linked issue.
- If actual time differs from the estimate by more than 2x, add a short variance note to the task body.

## Practical Workflow

1. Read the relevant PRD for product direction.
2. Use `.stint` for the operating graph: sprint order, blockers, estimates, timing, and task ownership.
3. Use GitHub issues for implementation detail, labels, prior attempts, and PR pipeline state.
4. When completing a GitHub issue, update every linked stint task that was materially worked.

## Priority

Tasks have an optional `priority` field: `p0` through `p4`.

| Level | Meaning |
|---|---|
| `p0` | On fire |
| `p1` | Shipping blocker |
| `p2` | Important, not blocking |
| `p3` | Polish |
| `p4` | Backlog |

Omitting priority means unprioritized (sorts after all prioritized tasks in `stint next`). Set via frontmatter (`priority: p2`) or at creation (`stint add "title" --priority p2`). Filter with `stint list --priority p0`. In `stint next`, priority breaks ties within sprint order.

## Blocker syntax

`blocked_by` is unified and polymorphic. Single field, accepts a string or list.

| Syntax | Meaning |
|---|---|
| bare integer | local stint task (auto-padded to 4 digits) |
| `@N` | local GitHub issue |
| `owner/repo@N` | external GitHub issue |
| `owner/repo:NNNN` | task in an external GitHub repo |
| `../path:NNNN` | task in a sibling local directory |
| `../path@N` | issue in a sibling local directory |
| quoted string | free-text blocker note |

## Gotchas

- Do not treat `.stint` as one-to-one with GitHub issues. One task can link multiple issues, and one issue may require multiple tasks if it spans distinct lanes.
- Do not make priority decisions from issue age or old labels alone; check the PRD and current sprint graph.
- Do not overwrite an existing `started_at` unless correcting bad timing data.
- Do not archive instead of completing just to avoid timing. Archive is for work intentionally removed from the plan.
