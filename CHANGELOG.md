# Changelog

Newest releases appear first.
## [0.3.12] - 2026-07-12

### Changes
- feat: stint check <id> validates a single task by ID (0014)
- feat: add stint unclaim — revert in-progress task to todo, clearing started_at
## [0.3.11] - 2026-07-11

### Changes
- feat: add --area/--tags/--blocked-by/--gh-issue/--body-file/--no-edit to add, and headless `stint set`
## [0.3.10] - 2026-07-10

### Changes
- feat: add size field (S/M/L) to task schema
- Test shorthand blocker pruning
## [0.3.9] - 2026-07-02

### Changes
- Prune completed blocker refs on done
- feat: resolve direct task-path blockers (#10)
## [0.3.8] - 2026-07-02

### Changes
- feat: resolve direct task-path blockers (stint 0015)

## [0.3.6] - 2026-06-24

### Changes
- fix: make next show parallel-safe queue
- docs: clarify patch release workflow
## [0.3.5] - 2026-06-24

### Changes
- fix: keep bottlenecks claimable
## [0.3.4] - 2026-06-24

### Changes
- feat: accept uppercase priority values (P0-P4) in addition to lowercase
- Rework TUI around claim launcher
## [0.3.3] - 2026-06-20

### Changes
- feat: add priority sort mode to TUI
## [0.3.2] - 2026-06-20

### Changes
- feat: replace start with claim; remove --claim/--count from next
## [0.3.1] - 2026-06-20

### Changes
- feat: hide blocked tasks from stint next output
- docs: add priority section to stint skill
- docs: document priority field (P0-P4) in README, PRD, and CLAUDE.md
- feat: add first-class priority field (P0-P4) to tasks
## [0.2.10] - 2026-06-15

### Changes
- feat: space toggle follows task when it was the last item in its board column
## [0.2.9] - 2026-06-14

### Changes
- feat: add zsh shell completions via `stint completions zsh`
## [0.2.8] - 2026-06-14

### Changes
- feat: make sprint index the sole source of truth for sprint membership (#8)
## [0.2.7] - 2026-06-14

### Changes
- feat: add Rules 13+14 to enforce sprint index/frontmatter bidirectional consistency
- Update task statuses: complete interim TUI, archive sprint board
## [0.2.6] - 2026-06-14

### Changes
- feat: improve tui table task visibility
- Add stint agent skill and symlink install instructions
## [0.2.5] - 2026-06-14

### Changes
- Simplify architecture docs and clarify single-crate layout
- Fix changelog release boundaries
- Clarify docs-only release bump rule
## [0.2.4] - 2026-06-14

### Changes
- Ignore chores in changelog
- Add git-cliff release flow
- Add crates install update flow
- Add agent symlink and patch bump rule
- Polish TUI shortcuts and command palette
- Improve TUI footer and empty selection handling
- Bump version to 0.2.0
- Add ratatui TUI and e2e driver
- docs: add ratatui/crossterm/notify doc links + framework rationale to TUI spec
- docs: flesh out TUI spec — resolve ambiguities, drop live timer
- docs: add TUI_DESIGN.md best-case design; link 0013/0004
- docs: add 0013 PRD for interim ratatui TUI
- feat: legible stint next (blocked reasons, color, backlog label) (#7)
- feat: unified STATE column in stint list (#6)
- refactor: sprint entries are markdown links; drop the relink command (#5)
- feat: sprint files link to task files for editor `gf` navigation (#4)
- Single task-state classifier, cut gates, enforce state machine (#3)
- feat: backlog as true icebox (stint 0012) (#2)
- Add init command with GitHub import
- Improve next and done task output
- feat: add inherited blocker gates
- Improve blocked task visibility and validation
- Add --count and --json to stint next; atomic claim lock
- Unify blocked_by into a single polymorphic field
- Prioritize CLI-backed Plexi interface task
- Add task remove command aliases
- Hide completed tasks from default list
- Refine init GitHub import task
- Add task timing commands
