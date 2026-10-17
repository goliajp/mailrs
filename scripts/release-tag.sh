#!/usr/bin/env bash
# release-tag.sh — cut a tag through git flow and push it, which starts
# the workflow for that kind of tag:
#
#   v<X.Y.Z>                 release.yml (gate + multi-arch image + t02
#                            deploy + GitHub Release)
#   web-v<YYYY.MM.DD>-<n>    release-web.yml (bun check + test, build
#                            web/dist, rsync it to t02; no restart)
#
# Usage: ./scripts/release-tag.sh v2.9.31
#        ./scripts/release-tag.sh web-v2026.10.18-1
#        ./scripts/release-tag.sh v2.9.31 --dry-run   # print, touch nothing
#
# The everyday release is ./scripts/direct-deploy.sh; this lane is for
# when a multi-arch image and a GitHub Release are wanted.
#
# release.yml's own `gate` job (full test suite) is what stands between
# the tag and production.
#
# Pushing a tag starts a deploy pipeline, so it asks first, and
# --dry-run exists to see what it would do.
set -euo pipefail

TAG="${1:?usage: release-tag.sh v<X.Y.Z>|web-v<YYYY.MM.DD>-<n> [--dry-run]}"
DRY_RUN=0
[ "${2:-}" = "--dry-run" ] && DRY_RUN=1
cd "$(dirname "$0")/.."

if [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    WORKFLOW=release.yml
elif [[ "$TAG" =~ ^web-v[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9]+$ ]]; then
    WORKFLOW=release-web.yml
else
    echo "!! tag must look like v1.2.3 or web-v2026.10.18-1 (got '$TAG')"
    exit 1
fi

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
LIVE="$(gh run list --workflow "$WORKFLOW" --limit 20 --json headBranch,status \
    --jq '[.[] | select(.status=="in_progress" or .status=="queued")] | .[0].headBranch' \
    2>/dev/null || true)"
if [ -n "$LIVE" ] && [ "$LIVE" != "null" ]; then
    echo "!! $WORKFLOW is still running for $LIVE — wait for it or cancel it first"
    exit 1
fi

echo "==> $TAG from develop @ $(git rev-parse --short HEAD) — starts $WORKFLOW"

if [ "$DRY_RUN" = 1 ]; then
    echo "==> dry run — would git flow release $TAG and push master, develop and tags"
    exit 0
fi

printf '==> about to tag %s and push (starts %s). Continue? [y/N] ' "$TAG" "$WORKFLOW"
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

echo "==> [2/2] push — this starts $WORKFLOW"
git push origin master develop --tags

cat <<EOF

$TAG is building. Watch it with:

  gh run watch \$(gh run list --workflow $WORKFLOW --limit 1 --json databaseId --jq '.[0].databaseId')
EOF
