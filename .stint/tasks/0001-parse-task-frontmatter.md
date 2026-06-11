---
id: "0001"
title: "Parse task frontmatter"
status: done
estimate: "3h"
actual: "2.5h"
area:
  - "core"
tags:
  - "parsing"
blocked_by: []
gh_issue: []
---

## Why

Tasks are stored as markdown files with YAML frontmatter. The core library needs
to parse and validate this format at load time, normalising polymorphic fields
(string-or-list) to Vec<T>.

## Gotchas

YAML scalars vs sequences must be handled via serde untagged enum — do not assume
the field is always a list.
