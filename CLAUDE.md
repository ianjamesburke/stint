# Stint — Agent Instructions

## What This Is

Stint is a Rust CLI tool + library for git-tracked, markdown-backed sprint planning. Read `NORTH_STAR.md` for vision, `docs/PRD.md` for full spec.

## Crate Responsibilities

- `stint-core` — pure library. Schema types, YAML frontmatter parsing, validation, mutation. Zero I/O side effects. All business logic lives here.
- `stint-cli` — thin binary. Clap command definitions, I/O, calls into core. No business logic.

## Key Invariants

- `stint-core` must have zero I/O. All file operations are the CLI's job.
- `blocked_by`, `blocked_by_gh`, `gh_issue`, `area`, `tags` are all polymorphic: accept string or list in YAML, always stored as `Vec<T>` internally.
- `stint check` is the source of truth for schema validity. Add a new field = add a new check rule.
- Duration strings: `h` for hours, `m` for minutes, decimals allowed ("1.5h", "30m"). Parse at the core level.

## Build

```bash
cargo build
cargo test
```

No `just` file yet — use cargo directly.

## Releases

- After any user-facing behavior change or installed local build, bump the patch version before committing.
- Use `just bump [patch|minor|major]` for release bumps. It updates `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md`, then commits `chore: release vX.Y.Z`.

## Source of Truth

- Schema spec → `docs/PRD.md` (Frontmatter schema section)
- CLI commands → `docs/PRD.md` (CLI Commands section)
- Check rules → `docs/PRD.md` (stint check Validation Rules section)
- Vision → `NORTH_STAR.md`
