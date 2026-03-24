#!/usr/bin/env bash
# usage: ./scripts/release.sh [--web-only] [patch|minor|major|<version>]
# --web-only: skip Rust tests and cross-compilation, only deploy web assets
# runs tests, bumps version, commits, and deploys
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# parse flags
WEB_ONLY=false
BUMP="patch"
for arg in "$@"; do
  case "$arg" in
    --web-only) WEB_ONLY=true ;;
    *) BUMP="$arg" ;;
  esac
done

# read current version from root Cargo.toml
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

# 1. rust tests (skip in web-only mode)
if [ "$WEB_ONLY" = false ]; then
  echo "==> running cargo test"
  cargo test --workspace
  echo ""
fi

# 2. web lint + tests
echo "==> running web check"
(cd web && bun run check)
echo ""
echo "==> running web tests with coverage"
(cd web && bun run test:coverage)
echo ""

# 3. check working tree is clean (except version files)
if ! git diff --quiet --ignore-submodules -- ':!Cargo.toml' ':!web/package.json' ':!Cargo.lock'; then
  echo "error: uncommitted changes found (other than version files)"
  echo "commit or stash your changes before releasing"
  exit 1
fi

# 4. bump version
echo "==> bumping version to $VERSION"
"$ROOT/scripts/bump.sh" "$VERSION"

# 5. commit
echo "==> committing version bump"
git add Cargo.toml Cargo.lock web/package.json
git commit -m "chore: bump version to $VERSION"

# 6. tag
echo "==> tagging v$VERSION"
git tag "v$VERSION"

# 7. push
echo "==> pushing to origin"
git push && git push --tags

# 8. deploy
echo "==> deploying"
if [ "$WEB_ONLY" = true ]; then
  "$ROOT/scripts/deploy.sh" --web-only
else
  "$ROOT/scripts/deploy.sh"
fi

echo ""
echo "==> released v$VERSION"
