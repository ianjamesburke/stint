# GOTCHAS

Non-obvious discoveries for this repo — things that cost real debugging time and
are not captured in the code, tests, or commit messages. Add an entry the moment
you hit one; this file is the repo's shared debugging memory.

Format per entry:

## <short title>
- **Symptom:** what you saw
- **Cause:** the real reason
- **Fix / avoid:** what to do

## Every worktree has its own `.stint/tasks/`, so task files cannot coordinate IDs

- **Symptom:** two agents in different worktrees filed tasks with the same ID —
  in one session, a whole nine-ID block overlapped. In narrator-ai-v1 the root
  tree had 165 task files while a feature worktree had 124.
- **Cause:** `git worktree` gives each worktree its own checkout of
  `.stint/tasks/`. An untracked task file in one worktree is structurally
  invisible to every other worktree; git tracking was the only channel between
  them, which is why committing appeared to fix it.
- **Fix / avoid:** IDs come from a ledger in the common git dir
  (`git rev-parse --git-common-dir`), shared by all worktrees and outside every
  checkout — see `src/idspace.rs`. Never reintroduce numbering that depends on
  task files or on `.stint/` being committed. `stint doctor` reconciles a cold
  or stale ledger against every worktree and ref.

## `.stint/` is git-ignored in this repo

- **Symptom:** `stint add` inside a `worktrees/...` checkout writes to the main
  checkout's `.stint/`, and nothing about the plan is committed.
- **Cause:** `.gitignore` line 6 ignores `.stint/`, so worktrees have no
  `.stint/` of their own and `StintRepo::find` walks up to the parent repo.
- **Fix / avoid:** expected for stint's own dogfooding; `stint add` warns about
  it. Consuming repos should commit `.stint/`.
