---
id: "0005"
title: "Init flow with optional GitHub issue import"
status: todo
estimate: "3h"
area:
  - "cli"
tags:
  - "init"
  - "gh-integration"
blocked_by: []
gh_issue: []
---

## Why

New repos need one obvious command to start using stint. `stint init` should
create the `.stint/` layout and optional config, and `stint init --with-github`
should import existing open GitHub issues as local tasks so the repo can move
onto markdown-backed planning without manual copying.

## Shape

- `stint init` creates `.stint/tasks`, `.stint/sprints`, and `.stint/config.toml`
- refuse by default if `.stint/` already exists
- `stint init --with-github` uses `gh` to import open issues
- `stint init --with-github --repo owner/name` overrides repo inference
- imported tasks get `status: backlog`, issue title/body, labels as tags, and
  `gh_issue`
- repeated imports should not duplicate issues already present in local tasks

## Gotchas

Requires `gh` CLI to be available and authenticated. Infer repo from git remote
if `.stint/config.toml` has no `[gh] repo` entry. Keep GitHub import as an
optional init path first; a later standalone sync/import command can reuse the
same importer.
