# Bump version, generate CHANGELOG via git-cliff, and commit. Defaults to patch.
#   just bump           # patch bump
#   just bump minor     # minor bump
#   just bump major     # major bump
bump bump="patch":
    bash scripts/release-version.sh "{{bump}}"

# Check that the crate can be packaged and published.
publish-check:
    cargo publish --dry-run

# Publish the current version to crates.io.
# Usually GitHub Actions does this after pushing main.
publish:
    cargo publish

# Check formatting and run clippy on all targets with warnings denied.
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings

# Install the local checkout for manual testing.
install:
    cargo install --path .

# Verify that the pi-stint npm package tests pass and packs cleanly.
pi-publish-check:
    node --test packages/pi-stint/test/*.test.js
    npm pack --dry-run ./packages/pi-stint

# Publish pi-stint to npm so `pi install npm:pi-stint` works everywhere.
pi-publish:
    node --test packages/pi-stint/test/*.test.js
    npm publish ./packages/pi-stint --access public

# Initialize an interactive scratch playground from an example project (defaults to 'demo').
# Usage:
#   just playground             # sets up examples/playground from examples/demo
#   just playground api-service # sets up examples/playground from examples/api-service
playground example="demo":
    @rm -rf examples/playground
    @mkdir -p examples/playground
    @cp -r examples/{{example}}/. examples/playground/
    @echo "Playground initialized in examples/playground (from examples/{{example}})"
    @echo ""
    @echo "Try running:"
    @echo "  cd examples/playground"
    @echo "  stint status"
    @echo "  stint next"
    @echo "  stint gate list"
    @echo "  stint check --strict"
    @echo ""
    @echo "Mess around freely! Run 'just playground' again anytime to reset."

# Plexi board (packages/plexi-stint): not part of the crate or its release gate.
# Pick the Plexi CLI explicitly, e.g. `just plexi=plexi-pr-123 plexi-test`.
plexi := "plexi"

# Print how to install the matching Stint CLI and the Plexi board package.
plexi-install-help:
    @sed -e "s/<version>/$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)/g" -e "s/<plexi_min>/$(sed -n 's/^plexi_min = "\(.*\)"/\1/p' packages/plexi-stint/manifest.toml)/g" docs/PLEXI_BOARD.md

# Run the board's own tests (app tests via Plexi, then real-stint process tests).
plexi-test:
    cargo build
    {{plexi}} app test packages/plexi-stint

# Validate and build the separate board package into target/plexi/.
# Stages a copy without tests or notes so the package ships only the app.
plexi-package:
    {{plexi}} app check packages/plexi-stint
    rm -rf target/plexi
    mkdir -p target/plexi/stage
    rsync -a --exclude dist --exclude tests --exclude __pycache__ --exclude .pytest_cache --exclude render-output --exclude AGENTS.md packages/plexi-stint/ target/plexi/stage/plexi-stint/
    {{plexi}} app package target/plexi/stage/plexi-stint --out target/plexi/plexi-stint-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1).plexipkg

# Prove the board stays out of the crate and the crate builds without Plexi or Python.
plexi-crate-check:
    @cargo package --list --allow-dirty | { ! grep -E '^(packages|plexi)' ; } && echo "ok: crate excludes packages/"
    cargo build
