# Stint — Agent Instructions

## What This Is

Stint is a Rust CLI for task and sprint tracking as markdown files in your repo. Read `NORTH_STAR.md` for vision, `docs/PRD.md` for full spec.

Single crate with `[lib]` and `[[bin]]` targets. Library modules handle schema, parsing, validation, mutation. `src/main.rs` is a thin Clap shell with no business logic.

## Key Invariants

- Library modules must have zero I/O. All file operations are `main.rs` / `cmds.rs` / `repo.rs`'s job.
- `blocked_by`, `blocked_by_gh`, `gh_issue`, `area`, `tags` are all polymorphic: accept string or list in YAML, always stored as `Vec<T>` internally.
- `status` lifecycle: `proposed` (draft idea / throwaway PRD outside pipeline), `backlog` (icebox), `todo` (committed pipeline), `in-progress` (claimed), `done`, `archived`.
- `stint approve <id>` gates promotion from `proposed`/`backlog` to `todo` on pass criteria: `priority`, `size`, `## Why`, and `## Done When`.
- Decision gates (`tags: [gate]`) require all 7 sections (`Ask`, `Options`, `Good answer`, `Test`, `Evidence`, `Links`, `Decision`), validated by `stint check`.
- `stint guide` and `stint schema` are the built-in operational and schema references for agents and humans.
- `stint check` (`--strict`) is the source of truth for schema and pass-criteria validity. Add a new field = add a new check rule.
- Duration strings: `h` for hours, `m` for minutes, decimals allowed ("1.5h", "30m"). Parse at the core level.

## Build & Test

```bash
cargo build
cargo test
node --test packages/pi-stint/test/*.test.js
just publish-check
```

Use `just --list` for release/install helper commands.

## After Every Code Change

Run `just install` immediately after any Rust change so the local binary (`~/.cargo/bin/stint`) is updated. Bare `stint` runs `stint status`; interactive visualization and command flows live in the Pi extension at `packages/pi-stint`.

## Releases

- Bump the patch version after Rust code changes that affect the crate/binary behavior, or after installing a local build for testing a Rust change.
- Do not bump the patch version for docs-only, comments-only, changelog-only, or agent-instruction-only changes.
- If a change needs a patch bump, first commit the approved behavior change, then run `just bump patch`, `git push origin main --follow-tags`, and `just install` so the published repo state and local binary match.
- Use `just bump [patch|minor|major]` for release bumps. It updates `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md`, commits `chore: release vX.Y.Z`, and tags `vX.Y.Z`.
- Push release commits with `git push origin main --follow-tags` so future changelog generation has a previous-version boundary.

## Source of Truth

- Built-in guide & schema → `stint guide` / `stint schema` (`src/guide.rs`)
- Schema spec → `docs/PRD.md` (Frontmatter schema section)
- CLI commands → `docs/PRD.md` (CLI Commands section)
- Check rules → `docs/PRD.md` (stint check Validation Rules section)
- Vision → `NORTH_STAR.md`
