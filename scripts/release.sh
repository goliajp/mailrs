#!/usr/bin/env bash
# usage: ./scripts/release.sh [--web-only|--ci|--ghcr] [patch|minor|major|<version>]
# --web-only: skip Rust tests and cross-compilation, only deploy web assets
# --ci:       skip local deploy; commit + tag + push so CI takes over
#             (`release.yml` builds + pushes ghcr image; remote prod still
#             needs to pull the image — see CI-SETUP.md / ROADMAP v5 L2 #4).
# --ghcr:     deploy via ghcr-pull instead of local cargo zigbuild. Requires
#             the target version's ghcr image to be published already
#             (release.yml runs on tag push; for a fresh version use --ci
#             first to publish, then run again with --ghcr). The standard
#             local path stays the rollback fallback until v5 closes.
# runs tests, bumps version, deploys, then tags and pushes only on success.
# if deploy fails, the local version bump is rolled back so the working tree
# stays clean and the next attempt starts from the same base.
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

# at most one of --ci / --ghcr / --web-only
mode_count=0
[ "$CI_MODE" = true ]    && mode_count=$((mode_count + 1))
[ "$GHCR_MODE" = true ]  && mode_count=$((mode_count + 1))
[ "$WEB_ONLY" = true ]   && mode_count=$((mode_count + 1))
if [ "$mode_count" -gt 1 ]; then
  echo "error: --ci, --ghcr, and --web-only are mutually exclusive"
  echo "  --web-only: web assets only, local deploy"
  echo "  --ci:       skip local deploy; CI builds + pushes image"
  echo "  --ghcr:     local deploy via ghcr-pull (no cargo zigbuild)"
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
echo "==> running web tests"
# bun runtime's node:inspector lacks the v8 Coverage API that
# @vitest/coverage-v8 needs (https://github.com/oven-sh/bun/issues/…).
# Run plain `vitest run` until the web rewrite resolves this.
(cd web && bun run test)
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

# --ghcr mode has a chicken-and-egg: deploy needs the ghcr image, but the
# image is only built when the tag is pushed. So in --ghcr we commit + tag
# + push FIRST (triggering release.yml), wait for the multi-arch build to
# finish, then deploy from the freshly-published image. Legacy and --ci
# modes keep the original ordering (deploy → commit → push for legacy;
# commit → push only for --ci).
if [ "$GHCR_MODE" = true ]; then
  echo "==> --ghcr mode: commit + tag + push first to trigger release.yml"
  git add Cargo.toml Cargo.lock web/package.json
  git commit -m "chore: bump version to $VERSION"
  git tag "v$VERSION"
  git push && git push --tags

  echo "==> waiting for release.yml multi-arch build to publish ghcr.io/goliajp/mailrs:$VERSION"
  echo "    (this typically takes 15-25 min; sleeping 30s between polls)"
  # Give the tag-push webhook time to register a workflow run
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
  # poll until completed (timeout 50 min)
  for i in $(seq 1 100); do
    STATUS=$(gh run view "$RUN_ID" --json status -q .status 2>/dev/null || echo "")
    [ "$STATUS" = "completed" ] && break
    sleep 30
  done
  CONCLUSION=$(gh run view "$RUN_ID" --json conclusion -q .conclusion 2>/dev/null || echo "unknown")
  if [ "$CONCLUSION" != "success" ]; then
    echo "error: release.yml run $RUN_ID concluded $CONCLUSION — image may not be in ghcr" >&2
    echo "       fix the CI failure, then run: ./scripts/deploy.sh --ghcr" >&2
    exit 1
  fi
  echo "==> release.yml succeeded — image is in ghcr"

  echo "==> deploying ghcr image to prod"
  "$ROOT/scripts/deploy.sh" --ghcr
elif [ "$CI_MODE" = true ]; then
  echo "==> --ci mode: skipping local deploy (CI release.yml will build + push ghcr image)"
  echo ""
  echo "==> committing version bump"
  git add Cargo.toml Cargo.lock web/package.json
  git commit -m "chore: bump version to $VERSION"
  echo "==> tagging v$VERSION"
  git tag "v$VERSION"
  echo "==> pushing to origin"
  git push && git push --tags
else
  # legacy build-from-source path
  trap rollback_bump EXIT

  echo "==> deploying"
  if [ "$WEB_ONLY" = true ]; then
    "$ROOT/scripts/deploy.sh" --web-only
  else
    "$ROOT/scripts/deploy.sh"
  fi

  # deploy succeeded — disarm the rollback trap and persist the bump
  trap - EXIT

  echo ""
  echo "==> committing version bump"
  git add Cargo.toml Cargo.lock web/package.json
  git commit -m "chore: bump version to $VERSION"

  echo "==> tagging v$VERSION"
  git tag "v$VERSION"

  echo "==> pushing to origin"
  git push && git push --tags
fi

echo ""
if [ "$CI_MODE" = true ]; then
  echo "==> released v$VERSION (tag pushed; CI release.yml takes over)"
  echo "    watch: gh run watch \$(gh run list --workflow=release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
  echo "    NOTE: remote prod is NOT auto-updated yet. To deploy manually after CI green:"
  echo "      ssh \$SSH_HOST 'docker pull ghcr.io/goliajp/mailrs:$VERSION && docker compose -f docker-compose.prod.yml up -d'"
else
  echo "==> released v$VERSION"
fi
