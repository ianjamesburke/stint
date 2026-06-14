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

# Install the local checkout for manual testing.
install:
    cargo install --path .
