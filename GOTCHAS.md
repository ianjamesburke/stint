# GOTCHAS

Non-obvious discoveries for this repo — things that cost real debugging time and
are not captured in the code, tests, or commit messages. Add an entry the moment
you hit one; this file is the repo's shared debugging memory.

Format per entry:

## <short title>
- **Symptom:** what you saw
- **Cause:** the real reason
- **Fix / avoid:** what to do

## Task IDs collide when `.stint/tasks/` is only partly committed

- **Symptom:** two agents on different branches or worktrees file tasks that
  share an ID — in one session, a whole nine-ID block overlapped.
- **Cause:** auto-numbering read only the current working directory. Task files
  routinely sit untracked, staged-but-uncommitted, or on an unmerged branch, so
  each branch computed the same "next" ID against a partial view and neither
  noticed.
- **Fix / avoid:** `src/idspace.rs` surveys every worktree on disk, every local
  and remote-tracking ref, and the `.stint/ids/` ledger before numbering, and
  reserves the ID with an exclusive create. Commit task files promptly anyway
  (`stint add --commit`) — a task that exists only in one working tree is
  invisible to every other clone, and `stint` can only warn about that.

## `.stint/` is git-ignored in this repo

- **Symptom:** `stint add` inside a `worktrees/...` checkout writes to the main
  checkout's `.stint/`, and nothing about the plan is committed.
- **Cause:** `.gitignore` line 6 ignores `.stint/`, so worktrees have no
  `.stint/` of their own and `StintRepo::find` walks up to the parent repo.
- **Fix / avoid:** expected for stint's own dogfooding; `stint add` warns about
  it. Consuming repos should commit `.stint/`.
