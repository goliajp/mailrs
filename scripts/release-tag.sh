#!/usr/bin/env bash
# release-tag.sh — cut a v* tag through git flow and push it, which
# starts release.yml (gate + multi-arch image + t02 deploy + GitHub
# Release).
#
# Usage: ./scripts/release-tag.sh v2.9.31
#        ./scripts/release-tag.sh v2.9.31 --dry-run   # print, touch nothing
#
# The everyday release is ./scripts/direct-deploy.sh; this lane is for
# when a multi-arch image and a GitHub Release are wanted.
#
# This script used to re-stamp the staging soak verdict on t01 so that
# release.yml's staging gate could match the tag's commit. That gate
# was removed from release.yml on 2026-07-21 — staging shares t01 with
# other projects and failed releases on its neighbours' noise — so
# nothing here touches staging any more. release.yml's own `gate` job
# (full test suite) is what stands between the tag and production.
#
# Pushing a tag starts a deploy pipeline, so it asks first, and
# --dry-run exists to see what it would do.
set -euo pipefail

TAG="${1:?usage: release-tag.sh v<X.Y.Z> [--dry-run]}"
DRY_RUN=0
[ "${2:-}" = "--dry-run" ] && DRY_RUN=1
cd "$(dirname "$0")/.."

case "$TAG" in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "!! tag must look like v1.2.3 (got '$TAG')"; exit 1 ;;
esac

if [ "$(git branch --show-current)" != develop ]; then
    echo "!! release from develop (on '$(git branch --show-current)')"
    exit 1
fi

if [ -n "$(git status --porcelain)" ]; then
    echo "!! working tree dirty — commit first"
    exit 1
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    echo "!! tag $TAG already exists locally"
    exit 1
fi

# Two releases racing each other would deploy t02 twice, in an order
# nobody chose.
LIVE="$(gh run list --workflow release.yml --limit 20 --json headBranch,status \
    --jq '[.[] | select(.status=="in_progress" or .status=="queued")] | .[0].headBranch' \
    2>/dev/null || true)"
if [ -n "$LIVE" ] && [ "$LIVE" != "null" ]; then
    echo "!! release.yml is still running for $LIVE — wait for it or cancel it first"
    exit 1
fi

echo "==> $TAG from develop @ $(git rev-parse --short HEAD)"

if [ "$DRY_RUN" = 1 ]; then
    echo "==> dry run — would git flow release $TAG and push master, develop and tags"
    exit 0
fi

printf '==> about to tag %s and push (starts release.yml). Continue? [y/N] ' "$TAG"
read -r reply </dev/tty
case "$reply" in
    y | Y) ;;
    *) echo "aborted"; exit 1 ;;
esac

echo "==> [1/2] git flow release $TAG"
git flow release start "$TAG" >/dev/null
GIT_MERGE_AUTOEDIT=no git flow release finish -m "Release $TAG" "$TAG" >/dev/null
echo "    $TAG -> $(git rev-list -n1 "$TAG")"
git checkout -q develop

echo "==> [2/2] push — this starts release.yml"
git push origin master develop --tags

cat <<EOF

$TAG is building. Watch it with:

  gh run watch \$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
EOF
