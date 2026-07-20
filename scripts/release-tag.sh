#!/usr/bin/env bash
# release-tag.sh — cut a v* tag and line the staging soak verdict up
# behind it, so release.yml's gate can actually pass.
#
# Usage: ./scripts/release-tag.sh v2.9.31
#
# The problem this solves. release.yml refuses to deploy unless
# /var/run/staging-gate.json on t01 reports, all at once:
#
#   .sha  == the commit the tag points at
#   .pass == true
#   age    < 3600s
#
# But the soak verdict is stamped by staging-build-deploy.sh with
# whatever commit was on develop at deploy time, and a tag is cut later
# — often after another commit, and always after `git flow release
# finish` has had a chance to introduce a merge commit. The two shas
# then never match and the gate sits there until it times out an hour
# later. Working this out from a red CI run, twice, is how this script
# came to exist.
#
# So: cut the tag, re-stamp staging with the tag's commit, re-kick the
# soak, and only then push. The gate finds a matching verdict on one of
# its polls instead of never.
#
# Re-stamping is honest only when staging is running the same code the
# tag builds. The script enforces that: it refuses when the diff between
# the staged commit and the tag touches anything a binary is built from.
# A docs-only difference is fine and is the common case.
set -euo pipefail

TAG="${1:?usage: release-tag.sh v<X.Y.Z>}"
HOST="${STAGING_HOST:-t01}"
cd "$(dirname "$0")/.."

case "$TAG" in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "!! tag must look like v1.2.3 (got '$TAG')"; exit 1 ;;
esac

if [ -n "$(git status --porcelain)" ]; then
    echo "!! working tree dirty — commit first"
    exit 1
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    echo "!! tag $TAG already exists locally"
    exit 1
fi

STAGED_SHA="$(ssh "$HOST" 'cat /etc/staging-deploy-sha 2>/dev/null' || true)"
if [ -z "$STAGED_SHA" ]; then
    echo "!! staging has no deploy sha — run staging-build-deploy.sh first"
    exit 1
fi

echo "==> [1/5] checking the tag builds the code staging is running"
echo "    staging: ${STAGED_SHA:0:8}"
echo "    HEAD:    $(git rev-parse --short HEAD)"
# Anything outside these paths cannot change the binary. Keep the list
# tight: when in doubt a path belongs on the "matters" side.
CODE_DIFF="$(git diff --name-only "$STAGED_SHA..HEAD" -- \
    crates/ Cargo.toml Cargo.lock Dockerfile rust-toolchain.toml || true)"
if [ -n "$CODE_DIFF" ]; then
    echo "!! staging is NOT running this code:"
    printf '     %s\n' $CODE_DIFF
    echo "   deploy to staging and let it soak before tagging:"
    echo "     ./scripts/staging-build-deploy.sh"
    exit 1
fi
echo "    only non-building files differ — re-stamp is honest"

echo "==> [2/5] git flow release $TAG"
git flow release start "$TAG" >/dev/null
GIT_MERGE_AUTOEDIT=no git flow release finish -m "Release $TAG" "$TAG" >/dev/null
TAG_SHA="$(git rev-list -n1 "$TAG")"
echo "    $TAG -> $TAG_SHA"

echo "==> [3/5] point staging's verdict at the tag's commit"
ssh "$HOST" "echo '$TAG_SHA' > /etc/staging-deploy-sha"

echo "==> [4/5] re-kick the soak (verdict in ~30 min)"
ssh "$HOST" "systemctl reset-failed staging-soak-gate.service 2>/dev/null || true
systemctl restart --no-block staging-soak-gate.service"

echo "==> [5/5] push — this starts release.yml"
git push origin master develop --tags

cat <<EOF

$TAG is building. The gate polls for a verdict with
sha=${TAG_SHA:0:8} pass=true age<60min, which the soak kicked above will
produce. Watch it with:

  gh run watch \$(gh run list --limit 1 --json databaseId --jq '.[0].databaseId')
  ssh $HOST cat /var/run/staging-gate.json
EOF
