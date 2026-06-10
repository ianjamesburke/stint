---
id: "0010"
title: "Generate Plexi interface from the stint CLI"
status: todo
estimate: "6h"
sprint: "s1"
area:
  - "cli"
  - "apps"
tags:
  - "plexi"
  - "ui"
blocked_by: []
blocked_by_gh: []
gh_issue: []
---

## Why

The Plexi interface should prove the flow where `stint` itself owns UI
generation and command behavior. Instead of building a separate Plexi app that
uses stint as a backend, the CLI should be able to produce/open a Plexi UI that
drives `stint` commands underneath.

## Shape

- add a CLI command that generates or opens the Plexi interface
- keep task data access behind existing `stint` commands and file formats
- make the generated UI usable via Plexi app open flow
- show current sprint tasks first
- support common actions: list, show, start, done, remove
- treat this as a proof of concept for CLI-owned Plexi app surfaces

## Gotchas

Do not introduce a second persistence model for the Plexi UI. The UI is a
surface over the CLI and `.stint/` files, not a replacement backend.
