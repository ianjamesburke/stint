#!/usr/bin/env bash
# Bump version, regenerate CHANGELOG via git-cliff, and commit.
# Usage: scripts/release-version.sh [patch|minor|major]
# Default: patch
set -euo pipefail

REPO_ROOT=$(dirname "$(git rev-parse --git-common-dir)")
TREE="$REPO_ROOT"

die() { echo "error: $*" >&2; exit 1; }

bump="${1:-patch}"
case "$bump" in
    patch|minor|major) ;;
    *) die "unknown bump type '$bump' - must be: patch | minor | major" ;;
esac

git -C "$TREE" diff --quiet && git -C "$TREE" diff --cached --quiet \
    || die "repo has uncommitted changes - commit first"

command -v git-cliff >/dev/null 2>&1 || die "git-cliff not found - brew install git-cliff"

current=$(grep '^version' "$TREE/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
base=$(echo "$current" | sed 's/-.*//')
IFS='.' read -r major minor patch <<< "$base"

case "$bump" in
    patch) new="$major.$minor.$((patch + 1))" ;;
    minor) new="$major.$((minor + 1)).0" ;;
    major) new="$((major + 1)).0.0" ;;
esac

echo "Bumping $current -> $new ($bump)..."

sed -i '' "s/^version = \"$current\"/version = \"$new\"/" "$TREE/Cargo.toml"
# Keep the Plexi board pinned to the CLI version (checked by packages/plexi-stint/tests).
board="$TREE/packages/plexi-stint"
sed -i '' "s/^version = \"$current\"/version = \"$new\"/" "$board/manifest.toml"
sed -i '' "s/^EXPECTED_STINT = \"$current\"/EXPECTED_STINT = \"$new\"/" "$board/app/model.py"
(cd "$TREE" && cargo generate-lockfile --quiet 2>/dev/null || cargo generate-lockfile)

echo "Generating changelog..."
touch "$TREE/CHANGELOG.md"
(cd "$TREE" && git-cliff \
    --config cliff.toml \
    --unreleased \
    --tag "v$new" \
    --prepend CHANGELOG.md)

git -C "$TREE" add Cargo.toml Cargo.lock CHANGELOG.md packages/plexi-stint/manifest.toml packages/plexi-stint/app/model.py
git -C "$TREE" commit -m "chore: release v$new"
git -C "$TREE" tag -a "v$new" -m "v$new"

echo ""
echo "v$new committed."
echo "Next: git push origin main --follow-tags"
