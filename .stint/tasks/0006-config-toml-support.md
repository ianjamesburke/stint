---
id: "0006"
title: "Read .stint/config.toml for CLI-backed Plexi UI"
status: backlog
estimate: "2h"
area:
  - "core"
  - "cli"
  - "apps"
blocked_by: []
blocked_by_gh: []
gh_issue: []
tags:
  - "config"
  - "plexi"
---

## Why

The CLI-backed Plexi UI needs a small amount of repo-local configuration:
default sprint, display preferences, and eventually app-open defaults. Keeping
that in `.stint/config.toml` lets `stint` generate a Plexi UI that still uses
the CLI as the backend instead of growing a separate Plexi app data layer.

## Shape

- parse optional `.stint/config.toml`
- support `default_sprint`
- reserve a `[plexi]` table for UI generation/open defaults
- keep all commands working when config is absent
- expose one internal config loader shared by CLI commands

## Gotchas

Config should refine defaults, not become required state. Missing optional
fields must fall back cleanly; malformed config should fail loudly with the
path and field name.
