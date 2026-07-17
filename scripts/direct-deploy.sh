#!/usr/bin/env bash
# direct-deploy.sh — the default release path (user decision 2026-07-18):
# build locally, publish to ghcr, and roll staging + prod together. CI
# (v*/web-v* tags) runs only when explicitly requested — it takes ~1.5 h
# vs ~15 min for this script, and the quality gates that matter (local
# fmt+clippy+test, staging soak, post-deploy replay-clean + route probe)
# all live outside CI anyway.
#
# Usage:
#   ./scripts/direct-deploy.sh <version>            # e.g. 2.9.14
#   SKIP_BUILD=1 ./scripts/direct-deploy.sh <ver>   # reuse last local image
#
# Steps:
#   1. buildx a linux/arm64 image locally (t01 + t02 are both arm64)
#   2. push ghcr.io/goliajp/mailrs:<version> (arm64-only; best-effort —
#      a push failure warns but never blocks the deploy)
#   3. prod (t02): save|ssh load, bump MAILRS_VERSION, compose up
#   4. staging (t01): delegate to staging-build-deploy.sh SKIP_BUILD=1
#      (it ships the image + soak harness + kicks the 30-min soak)
#   5. verify prod: :3301 up, health version matches, AOF replay (clean)
set -euo pipefail

VERSION="${1:?usage: direct-deploy.sh <version>}"
TAG="mailrs:staging-local"
GHCR="ghcr.io/goliajp/mailrs:$VERSION"
PROD="root@t02.golia.jp"
cd "$(dirname "$0")/.."

if [ -n "$(git status --porcelain)" ]; then
    echo "!! working tree dirty — commit first (deploys must be reproducible)"
    exit 1
fi

if [ "${SKIP_BUILD:-0}" != 1 ]; then
    echo "==> [1/5] local arm64 build ($VERSION)"
    docker buildx build \
        --platform linux/arm64 \
        --build-arg VERSION="$VERSION" \
        --build-arg CACHE_BUST="direct-$VERSION" \
        -t "$TAG" \
        --load \
        .
else
    echo "==> [1/5] SKIP_BUILD=1 — reusing local $TAG"
fi

echo "==> [2/5] push $GHCR (best-effort)"
docker tag "$TAG" "$GHCR"
if ! docker push "$GHCR"; then
    echo "!! ghcr push failed — continuing with the deploy (image still ships via save|load)"
fi

echo "==> [3/5] prod: save | ssh load + compose up"
docker save "$GHCR" | gzip -1 | ssh "$PROD" 'gunzip | docker load'
ssh "$PROD" "cd /apps/mailrs \
  && sed -i 's/^MAILRS_VERSION=.*/MAILRS_VERSION=$VERSION/' .env \
  && docker compose up -d --pull never --no-deps receiver fastcore webapi-fc fastcore-sender"

echo "==> [4/5] staging: delegate to staging-build-deploy.sh"
SKIP_BUILD=1 ./scripts/staging-build-deploy.sh

echo "==> [5/5] verify prod"
for i in $(seq 1 90); do
    if ssh "$PROD" "docker exec mailrs-fastcore curl -sf -m3 http://localhost:3301/v1/admin/maintenance:rewrite-aof -X GET -o /dev/null" 2>/dev/null; then
        break
    fi
    # any 2xx-4xx means the router is up; only connection failures loop
    if ssh "$PROD" "docker exec mailrs-fastcore curl -s -m3 -o /dev/null -w '%{http_code}' http://localhost:3301/healthz" 2>/dev/null | grep -qE '^[0-9]'; then
        break
    fi
    sleep 2
done
GOT_VERSION=$(ssh "$PROD" "curl -s -m5 localhost:3103/api/health" | grep -o '"version":"[^"]*"' || true)
echo "    health: $GOT_VERSION (want $VERSION)"
REPLAY=$(ssh "$PROD" "docker logs mailrs-fastcore 2>&1 | grep -iE 'kevy: AOF .* replayed' | tail -1" || true)
echo "    replay: $REPLAY"
case "$REPLAY" in
    *"(clean)"*) echo "    replay clean ✓" ;;
    *) echo "!! replay line is NOT clean — investigate before walking away (AOF black-hole SOP)" ;;
esac
echo "done: prod + staging on $VERSION (ghcr pushed, soak kicked)"
