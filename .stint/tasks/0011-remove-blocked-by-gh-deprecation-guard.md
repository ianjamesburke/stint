---
id: "0011"
title: "Remove blocked_by_gh deprecation guard"
status: done
estimate: "30m"
actual: "15m"
completed_at: "2026-06-10T22:24:23Z"
blocked_by: []
gh_issue: []
area:
  - "core"
tags:
  - "cleanup"
  - "frontmatter"
---


## Why

The parser currently carries an explicit `blocked_by_gh` field only to reject
it with a deprecation message. That keeps deprecated schema knowledge alive in
production parsing code and makes the frontmatter model less clean.

We do not want strict unknown-field validation, because task files may carry
harmless local metadata. The cleanup should remove the bespoke
`blocked_by_gh` guard while preserving permissive handling for unrelated
frontmatter keys.

## Shape

- remove `blocked_by_gh` from `RawFrontmatter`
- remove the explicit parser rejection branch and parser tests for it
- remove the CLI integration test that expects `stint check` to report
  `blocked_by_gh`
- leave serde permissive for unknown frontmatter keys
- keep unified `blocked_by` behavior unchanged

## Gotchas

Do not add `#[serde(deny_unknown_fields)]`. This is a cleanup of one explicit
deprecated-field guard, not a broader schema strictness change.
