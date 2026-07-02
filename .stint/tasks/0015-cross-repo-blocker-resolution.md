---
id: "0015"
title: "feat: resolve direct task-path blockers"
status: in-progress
priority: p0
estimate: "2h"
started_at: "2026-07-02T03:57:09Z"
blocked_by: []
gh_issue: []
area:
  - "core"
  - "cli/commands"
tags:
  - "v1"
  - "validation"
  - "tooling"
---


Direct task-file paths such as `../plexi/.stint/tasks/0337-pane-new-tab-from-anchor.md` are currently treated as free-text blockers. Agents need a first-class way to block local work on an explicit task file in another repo, validate that the referenced task exists, and have normal `stint next` keep the local task blocked until that task is done.

## Scope

- Extend `BlockedByRef` parsing to recognize direct local filesystem task paths pointing at `.stint/tasks/<id>-*.md`, including relative paths like `../plexi/.stint/tasks/0337-pane-new-tab-from-anchor.md`.
- Resolve direct task-path blockers in the normal `stint check`, `stint list`, `stint next`, and `stint status` flows; do not require a separate `--cross-repo` command.
- In normal `stint check`, validate that a direct task-path blocker exists, parses as a stint task, and has an id matching the filename prefix.
- Update active-blocker classification so a resolved direct task-path blocker stops blocking when the referenced task status is `done` or `archived`; missing, malformed, or non-done task files remain active blockers.
- Preserve existing behavior for free-text notes, GitHub issue refs, `../repo:NNNN`, and remote `owner/repo:*` refs.
- Add tests covering: direct `.stint/tasks/<id>-slug.md` paths, missing task path, malformed target task, done/archived target unblocking, non-done target blocking, and no-regression for free-text blockers.
- Update `docs/PRD.md` with direct task-path blocker syntax and explicitly note that `--cross-repo` remains unrelated/deferred.

## Non-Scope

- Network calls to GitHub or remote repositories.
- Walking sibling repositories or resolving `../repo:NNNN` refs.
- Implementing or expanding `stint check --cross-repo`.
- Automatically mutating downstream `blocked_by` fields when an external task is completed.
- Full transitive cross-repo graph visualization.

## Why

Multi-repo agent orchestration needs machine-visible blockers between explicit task files; direct paths are enough for the current workflow and avoid repo discovery or a special validation command.

## References

- `src/schema.rs` — `BlockedByRef` parsing and display.
- `src/state.rs` — active blocker classification.
- `src/check.rs` — normal validation rules.
- `docs/PRD.md` — blocker model documentation.
