#!/usr/bin/env bash
# usage: ./scripts/release.sh [--web-only] [--ci] [patch|minor|major|<version>]
# --web-only: skip Rust tests and cross-compilation, only deploy web assets
# --ci:       skip local deploy; commit + tag + push so CI takes over
#             (`release.yml` builds + pushes ghcr image; remote prod still
#             needs to pull the image — see CI-SETUP.md / ROADMAP v5 L2 #4).
# runs tests, bumps version, deploys, then tags and pushes only on success.
# if deploy fails, the local version bump is rolled back so the working tree
# stays clean and the next attempt starts from the same base.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WEB_ONLY=false
CI_MODE=false
BUMP="patch"
for arg in "$@"; do
  case "$arg" in
    --web-only) WEB_ONLY=true ;;
    --ci)       CI_MODE=true ;;
    *) BUMP="$arg" ;;
  esac
done

if [ "$CI_MODE" = true ] && [ "$WEB_ONLY" = true ]; then
  echo "error: --ci and --web-only are mutually exclusive"
  echo "  --web-only deploys web assets locally; --ci skips local deploy entirely"
  exit 1
fi

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
  echo "==> running cargo test (skip _under_budget — dev-profile noise)"
  # Mirror .github/workflows/release.yml gate: perf_gate *_under_budget
  # tests are calibrated for release profile on M-series. Dev profile
  # under `cargo test --workspace` parallel load borders the budgets
  # and yields flaky failures that block ship. Skip them here; CI
  # already skips them. To run perf gates explicitly:
  #   cargo test --release --workspace -- _under_budget
  cargo test --workspace -- --skip _under_budget
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

# 4. deploy — if this fails, rollback the local bump before exiting.
#    --ci mode skips local deploy entirely; commit + push happen unconditionally
#    and CI (`release.yml`) builds + pushes the ghcr image. Remote prod is NOT
#    updated by --ci mode until ROADMAP v5 L2 #4 (prod docker-compose pulls
#    ghcr image) lands.
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

if [ "$CI_MODE" = false ]; then
  trap rollback_bump EXIT

  echo "==> deploying"
  if [ "$WEB_ONLY" = true ]; then
    "$ROOT/scripts/deploy.sh" --web-only
  else
    "$ROOT/scripts/deploy.sh"
  fi

  # deploy succeeded — disarm the rollback trap and persist the bump
  trap - EXIT
else
  echo "==> --ci mode: skipping local deploy (CI release.yml will build + push ghcr image)"
fi

echo ""
echo "==> committing version bump"
git add Cargo.toml Cargo.lock web/package.json
git commit -m "chore: bump version to $VERSION"

echo "==> tagging v$VERSION"
git tag "v$VERSION"

echo "==> pushing to origin"
git push && git push --tags

echo ""
if [ "$CI_MODE" = true ]; then
  echo "==> released v$VERSION (tag pushed; CI release.yml takes over)"
  echo "    watch: gh run watch \$(gh run list --workflow=release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
  echo "    NOTE: remote prod is NOT auto-updated yet. To deploy manually after CI green:"
  echo "      ssh \$SSH_HOST 'docker pull ghcr.io/goliajp/mailrs:$VERSION && docker compose -f docker-compose.prod.yml up -d'"
else
  echo "==> released v$VERSION"
fi
