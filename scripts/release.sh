#!/usr/bin/env bash
# usage: ./scripts/release.sh [--web-only|--ci|--ghcr] [patch|minor|major|<version>]
#
# Strict git-flow release flow:
#
#  1. require current branch = develop (and clean working tree)
#  2. run tests / checks
#  3. `git flow release start v<version>`           ← creates release/v<version>
#  4. bump.sh + cargo metadata (refresh Cargo.lock)
#  5. commit "chore: bump version to <version>" on release branch
#  6. `git flow release finish -p -m "v<version>" v<version>`
#       → merges to main, tags v<version> on main, merges back to develop,
#         pushes main + develop + tags. Tag push triggers .github/workflows/
#         release.yml which builds + pushes the multi-arch ghcr image.
#  7. mode-specific deploy:
#       --web-only : `git flow release finish --notag` (no GHA), then
#                    deploy.sh --web-only (scp web/dist, no container restart, ~10s)
#       --ghcr     : wait for release.yml to publish ghcr image, then
#                    deploy.sh --ghcr
#       --ci       : skip deploy entirely (CI publishes image, prod pull manual)
#       (default)  : legacy build-from-source path (cargo zigbuild → scp binary)
#
# Why this matters: the previous flow pushed commits straight to develop and
# tagged from there. git-flow gives us:
#   - main = always the deployed tip (rollback target)
#   - develop = active integration (where new features land)
#   - release/v<x> branches = isolated bump + finish (no half-bumped develop)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WEB_ONLY=false
CI_MODE=false
GHCR_MODE=false
BUMP="patch"
for arg in "$@"; do
  case "$arg" in
    --web-only) WEB_ONLY=true ;;
    --ci)       CI_MODE=true ;;
    --ghcr)     GHCR_MODE=true ;;
    *) BUMP="$arg" ;;
  esac
done

mode_count=0
[ "$CI_MODE" = true ]    && mode_count=$((mode_count + 1))
[ "$GHCR_MODE" = true ]  && mode_count=$((mode_count + 1))
[ "$WEB_ONLY" = true ]   && mode_count=$((mode_count + 1))
if [ "$mode_count" -gt 1 ]; then
  echo "error: --ci, --ghcr, and --web-only are mutually exclusive" >&2
  exit 1
fi

# ---------- preflight: must be on develop, clean, git-flow available ----------

CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "develop" ]; then
  echo "error: releases must start from 'develop' (current: $CURRENT_BRANCH)" >&2
  echo "  → git checkout develop && git pull, then try again" >&2
  exit 1
fi

if ! git diff --quiet --ignore-submodules HEAD; then
  echo "error: working tree has uncommitted changes" >&2
  git status --short >&2
  exit 1
fi

if ! command -v git-flow >/dev/null 2>&1 && ! git flow version >/dev/null 2>&1; then
  echo "error: git-flow not installed. brew install git-flow-avh" >&2
  exit 1
fi

# ---------- version target ----------

CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"
case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ; VERSION="$MAJOR.$MINOR.$PATCH" ;;
  minor) MINOR=$((MINOR + 1)) ; PATCH=0 ; VERSION="$MAJOR.$MINOR.$PATCH" ;;
  major) MAJOR=$((MAJOR + 1)) ; MINOR=0 ; PATCH=0 ; VERSION="$MAJOR.$MINOR.$PATCH" ;;
  [0-9]*.[0-9]*.[0-9]*) VERSION="$BUMP" ;;
  *) echo "error: version must be patch|minor|major|<semver>" >&2 ; exit 1 ;;
esac

echo "==> current: $CURRENT → target: $VERSION"
echo ""

# ---------- 1. tests ----------

if [ "$WEB_ONLY" = false ]; then
  echo "==> cargo nextest run --workspace (skip _under_budget)"
  # Same gate as release.yml. *_under_budget perf-gate tests are M-series
  # calibrated; CI runners are too slow + variable. Run them explicitly via
  # `cargo test --release -- _under_budget` outside this script.
  cargo nextest run --workspace --no-fail-fast \
    --filterset 'not test(/_under_budget/)'
  cargo test --workspace --doc -- --skip _under_budget
  echo ""
fi

echo "==> bun check + test"
(cd web && bun run check && bun run test)
echo ""

# ---------- 2. git flow release start ----------

RELEASE_BRANCH="release/v$VERSION"
echo "==> git flow release start v$VERSION"
git flow release start "v$VERSION"

# now on $RELEASE_BRANCH

# ---------- 3. bump + commit on release branch ----------

echo "==> bumping version files"
"$ROOT/scripts/bump.sh" "$VERSION"
# bump.sh rewrites Cargo.toml + web/package.json but doesn't touch Cargo.lock.
# Run metadata so internal mailrs-* versions update too.
cargo metadata --format-version 1 > /dev/null

git add Cargo.toml Cargo.lock web/package.json
git commit -m "chore: bump version to $VERSION"

# ---------- 4. mode-specific finish + deploy ----------

# Rollback helper: if finish fails or deploy fails midway, drop the release
# branch + restore develop unchanged. main is not touched until finish
# succeeds (git-flow only fast-forwards / merges main at the very end), so
# this is safe.
rollback_release() {
  local rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "" >&2
    echo "==> release failed (exit $rc) — cleaning up release branch" >&2
    git checkout develop 2>/dev/null || true
    git branch -D "$RELEASE_BRANCH" 2>/dev/null || true
    # tag may have landed if `git flow release finish` got partway; remove it
    # locally so a retry doesn't trip "tag exists"
    git tag -d "v$VERSION" 2>/dev/null || true
  fi
  return $rc
}
trap rollback_release EXIT

if [ "$WEB_ONLY" = true ]; then
  echo "==> --web-only: git flow release finish --notag (no CI image build needed)"
  # No tag → release.yml not triggered. Web bind-mount picks up the new
  # files on next request once deploy.sh scp's them.
  GIT_MERGE_AUTOEDIT=no git flow release finish -p -n -m "Release v$VERSION (web-only)" "v$VERSION"

  trap - EXIT
  echo ""
  echo "==> deploying web assets (scp; no container restart)"
  "$ROOT/scripts/deploy.sh" --web-only
  echo ""
  echo "==> released v$VERSION (web-only, no tag)"
  exit 0
fi

# All non-web-only paths finish with tag → that's the GHA trigger.
echo "==> git flow release finish v$VERSION (merge to main, tag, merge back to develop, push all)"
GIT_MERGE_AUTOEDIT=no git flow release finish -p -m "Release v$VERSION" "v$VERSION"

trap - EXIT

if [ "$CI_MODE" = true ]; then
  echo ""
  echo "==> released v$VERSION (--ci: tag pushed; CI release.yml takes over)"
  echo "    watch: gh run watch \$(gh run list --workflow=release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
  echo "    manual deploy after CI green:"
  echo "      ssh \$SSH_HOST 'sed -i \"s|MAILRS_VERSION=.*|MAILRS_VERSION=$VERSION|\" /apps/mailrs/.env && cd /apps/mailrs && docker compose up -d'"
  exit 0
fi

if [ "$GHCR_MODE" = true ]; then
  echo ""
  echo "==> waiting for release.yml to publish ghcr.io/goliajp/mailrs:$VERSION"
  echo "    typically 15-25 min; polling every 30s"
  sleep 15
  RUN_ID=""
  for _ in $(seq 1 10); do
    RUN_ID=$(gh run list --workflow=release.yml --branch "v$VERSION" --limit 1 --json databaseId -q '.[0].databaseId' 2>/dev/null || echo "")
    [ -n "$RUN_ID" ] && [ "$RUN_ID" != "null" ] && break
    sleep 5
  done
  if [ -z "$RUN_ID" ] || [ "$RUN_ID" = "null" ]; then
    echo "error: could not find release.yml run for tag v$VERSION" >&2
    exit 1
  fi
  echo "    watching run $RUN_ID"
  for _ in $(seq 1 100); do
    STATUS=$(gh run view "$RUN_ID" --json status -q .status 2>/dev/null || echo "")
    [ "$STATUS" = "completed" ] && break
    sleep 30
  done
  CONCLUSION=$(gh run view "$RUN_ID" --json conclusion -q .conclusion 2>/dev/null || echo "unknown")
  if [ "$CONCLUSION" != "success" ]; then
    echo "error: release.yml run $RUN_ID concluded $CONCLUSION" >&2
    echo "       fix CI then: ./scripts/deploy.sh --ghcr" >&2
    exit 1
  fi
  echo "==> release.yml succeeded — image is in ghcr"
  "$ROOT/scripts/deploy.sh" --ghcr
  echo ""
  echo "==> released v$VERSION (--ghcr)"
  exit 0
fi

# legacy build-from-source path
echo ""
echo "==> deploying (legacy build-from-source)"
"$ROOT/scripts/deploy.sh"
echo ""
echo "==> released v$VERSION (legacy)"
