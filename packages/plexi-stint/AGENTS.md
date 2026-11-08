# plexi-stint Agent Contract

Separately built Plexi board for Stint (stint 0020). Not part of the `stint`
crate (`Cargo.toml` excludes `packages`) and not bundled with Plexi. Scaffolded
by `plexi app init`; authoring guide is `sdk/python/AUTHORING.md` in the Plexi repo.

## Rules

- All task data and changes go through the four `[[cli_operation]]` entries in
  `manifest.toml`. No terminal, no `.stint` file access, no second store. A test
  fails if app source mentions `subprocess`, `.stint` or file effects.
- Board version == stint CLI version == `EXPECTED_STINT` in `app/model.py`.
  `scripts/release-version.sh` bumps all three; `tests/test_release_pins.py` guards it.
- Priority changes are the guarded compare-and-set from stint 0021:
  `--priority=<new> --expect-priority=<shown|unset>`. Conflict data arrives as
  structured stdout on `nonzero_exit`; never parse stderr text.
- State rules live in `app/model.py` (pure, no SDK). `main.py` only maps events.
  Add behavior test-first in `tests/test_model.py`.

## Layout

- `main.py`: SDK event to model mapping, effects.
- `app/model.py`: state machine (version gate, refresh, conflict, uncertain, read-only).
- `app/ui.py`: pure view builders.
- `tests/test_model.py`: model rules. `tests/test_app.py`: AppHarness protocol tests.
  `tests/test_stint_integration.py`: real `stint` binary against a temp repo
  (`STINT_BIN`, else `target/debug/stint`, else PATH). `tests/test_release_pins.py`.

## Loop

- From the stint repo root: `just plexi=plexi-pr-<N> plexi-test`, `plexi-package`.
- Plexi-free subset: `python -m pytest packages/plexi-stint/tests --ignore=packages/plexi-stint/tests/test_app.py`.
- Render: `plexi-pr-<N> app render packages/plexi-stint --png --output <path>`.
  Never start or stop a Plexi host from here.
