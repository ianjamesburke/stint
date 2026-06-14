# Changelog

Newest releases appear first.
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
