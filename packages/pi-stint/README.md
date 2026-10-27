# pi-stint

Pi extension for [`stint`](https://github.com/ianburke/stint) — visual task board, proposal triage, human decision gate resolution, live status bar, and automatic system-prompt context injection.

## Features

- **Live Status Bar**: Displays active `in-progress` stints (`⚡ 0017 p0`), open decision gates (`🚧 1 gate`), ready queue (`▶ 2 ready`), and proposals (`💡 3 proposed`) directly in Pi's footer.
- **Automatic Agent Context**: Injects a compact `stint` section into `event.systemPromptOptions.sections.stint` on `before_agent_start` so agents know what's active, what's ready, what gates are open, and how to invoke `stint guide` without external skill files.
- **Interactive `/stint` Command**:
  - `/stint` or `/stint board` — Visual workspace board + interactive picker to view tasks, claim tasks, resolve decision gates, approve/reject proposals, and run strict validation.
  - `/stint show [id]` / `/stint view [id]` / `/stint <id>` — View any task's full formatted Markdown card (metadata, blockers, `## Why`, `## Done When`, `## References`) and trigger quick contextual actions (claim, mark done, approve, resolve gate).
  - `/stint status` — Markdown summary of active, ready, blocked, proposed, and open gates.
  - `/stint next` — View the prioritized ready queue.
  - `/stint add [title]` — Interactive wizard to create a ready (`status: todo`) task with priority, size, `## Why`, and `## Done When`.
  - `/stint claim [id]` — Claim top ready task or a specific task ID.
  - `/stint done <id> [actual]` — Mark a task done and prune completed blockers.
  - `/stint unclaim <id>` — Return an in-progress task to `todo`.
  - `/stint propose <title>` — File an idea in `status: proposed` outside the active pipeline.
  - `/stint approve [id]` — Promote a proposal to `todo` (prompts for missing `priority`, `size`, `## Why`, or `## Done When`).
  - `/stint reject <id> [reason]` — Reject and archive a proposal.
  - `/stint gate [id]` — Inspect a 7-section decision gate and resolve it interactively (`## Decision` + auto-unblock).
  - `/stint check` — Run `stint check --strict`.
  - `/stint guide [topic]` — Display the built-in operator guide.

## Installation

### Local / Monorepo Path

Add the package path to `~/.pi/agent/settings.json`:

```json
{
  "packages": [
    "/path/to/stint/packages/pi-stint"
  ]
}
```

### npm Distribution

```bash
pi install npm:pi-stint
```
