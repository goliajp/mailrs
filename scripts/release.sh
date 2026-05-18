#!/usr/bin/env bash
# usage: ./scripts/release.sh [--web-only] [patch|minor|major|<version>]
# --web-only: skip Rust tests and cross-compilation, only deploy web assets
# runs tests, bumps version, deploys, then tags and pushes only on success.
# if deploy fails, the local version bump is rolled back so the working tree
# stays clean and the next attempt starts from the same base.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WEB_ONLY=false
BUMP="patch"
for arg in "$@"; do
  case "$arg" in
    --web-only) WEB_ONLY=true ;;
    *) BUMP="$arg" ;;
  esac
done

CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ; VERSION="$MAJOR.$MINOR.$PATCH" ;;
  minor) MINOR=$((MINOR + 1)) ; PATCH=0 ; VERSION="$MAJOR.$MINOR.$PATCH" ;;
  major) MAJOR=$((MAJOR + 1)) ; MINOR=0 ; PATCH=0 ; VERSION="$MAJOR.$MINOR.$PATCH" ;;
  [0-9]*.[0-9]*.[0-9]*)  VERSION="$BUMP" ;;
  *) echo "error: argument must be patch|minor|major|<semver>" ; exit 1 ;;
esac

echo "==> current version: $CURRENT"
echo "==> target  version: $VERSION"
echo ""

# 1. tests
if [ "$WEB_ONLY" = false ]; then
  echo "==> running cargo test"
  cargo test --workspace
  echo ""
fi

echo "==> running web check"
(cd web && bun run check)
echo ""
echo "==> running web tests with coverage"
(cd web && bun run test:coverage)
echo ""

# 2. require fully clean working tree — bump.sh is about to touch Cargo.toml,
#    web/package.json, Cargo.lock, and the rollback-on-deploy-failure trap below
#    relies on `git checkout --` restoring those files to exactly their HEAD state.
#    if there were pre-existing uncommitted changes to those files we would
#    silently destroy them on rollback.
if ! git diff --quiet --ignore-submodules HEAD; then
  echo "error: working tree has uncommitted changes"
  git status --short
  echo ""
  echo "commit or stash everything before releasing"
  exit 1
fi

# 3. bump version (local files only — not committed yet)
echo "==> bumping version to $VERSION"
"$ROOT/scripts/bump.sh" "$VERSION"

# bump.sh only rewrites Cargo.toml + web/package.json; Cargo.lock keeps the
# previous mailrs-* internal versions until something rebuilds. Force a
# resolver run so the eventual commit captures both files in sync.
echo "==> syncing Cargo.lock"
cargo metadata --format-version 1 > /dev/null
echo ""

# 4. deploy — if this fails, rollback the local bump before exiting
rollback_bump() {
  local rc=$?
  if [ "$rc" -ne 0 ]; then
    echo ""
    echo "==> deploy failed (exit $rc); rolling back local version bump"
    git checkout -- Cargo.toml Cargo.lock web/package.json 2>/dev/null || true
    # if web/bun.lock was touched by check/test, leave it alone — bun.lock
    # changes are independent of the version bump.
  fi
  return $rc
}
trap rollback_bump EXIT

echo "==> deploying"
if [ "$WEB_ONLY" = true ]; then
  "$ROOT/scripts/deploy.sh" --web-only
else
  "$ROOT/scripts/deploy.sh"
fi

# 5. deploy succeeded — disarm the rollback trap and persist the bump
trap - EXIT

echo ""
echo "==> committing version bump"
git add Cargo.toml Cargo.lock web/package.json
git commit -m "chore: bump version to $VERSION"

echo "==> tagging v$VERSION"
git tag "v$VERSION"

echo "==> pushing to origin"
git push && git push --tags

echo ""
echo "==> released v$VERSION"
