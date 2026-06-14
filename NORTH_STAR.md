# Stint — North Star

## The One-Liner

**Your codebase knows what it's doing. Now it knows when.**

---

## What Stint Is

Stint is a task tracker that lives inside your repository as plain markdown files. One file per task, versioned with your code.

The name is intentional. A stint is a defined period of focused work. That is the atomic unit of software development, and it deserves a first-class representation on disk.

---

## The Problem

Every project management tool makes the same mistake: it lives outside the codebase. Issues in GitHub, tasks in Linear, sprints in Jira — none of it travels with the code. When you clone a repo, you get the code but not the plan. When you grep for a bug, you can't see what task it belongs to. When an agent reads your codebase, it has no idea what's in progress, what's blocked, or what the next six weeks look like.

This is backwards. The plan is part of the codebase. It should be versioned with it, diffed with it, reviewed with it, and archived with it.

---

## What Stint Is Not

- Not a replacement for GitHub Issues. GitHub Issues are for public discussion, external contributors, and cross-repo coordination. Stint is for your internal plan — the private thinking behind the work.
- Not a calendar app. Stint has no UI of its own (yet).
- Not a project management platform. It is a plain text database. The intelligence lives in the CLI and the agents that read it.

---

## Core Beliefs

**Tasks are documents, not rows.** A task file contains the full thinking behind the work — why it exists, what was tried, what's blocked, relevant code references. The frontmatter is machine-readable metadata. The body is human-readable context. Both matter.

**Order is explicit, not inferred.** Sprint index files are ordered lists of task IDs. The order of lines is the priority order. Drag to reorder, git diff shows the history. No magic ranking algorithms, no priority scores — just a list in the order you decided.

**The CLI is the source of truth.** Every task is a plain markdown file. If the CLI disappeared, every task would still be readable in any text editor.

**Check early and often.** `stint check` validates the entire task graph: required fields, type correctness, cross-references, blocker resolution. Run it in CI. A broken task graph is a broken plan.

**GitHub is optional, not required.** Tasks can reference GitHub issues via `gh_issue` frontmatter. They don't have to. Stint works identically in a repo with no GitHub remote.

**Agents are first-class readers.** Task files are structured so an LLM can read `.stint/tasks/` and understand the full project state — what's open, what's blocked, what's in the current sprint, what was recently completed. This is intentional. Stint is designed to be the planning layer that agents read before they act.

---

## Long Horizon

Agents read sprint state before picking up tasks. Cross-repo blockers are resolved automatically. Any future interface (web, desktop, TUI) can be built on the same library.

The plan lives with the code. Always.
