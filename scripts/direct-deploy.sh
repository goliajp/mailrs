#!/usr/bin/env bash
# direct-deploy.sh — the default release path (user decision 2026-07-18):
# build locally, publish to ghcr, and roll prod. Staging was retired
# (v*/web-v* tags) runs only when explicitly requested — it takes ~1.5 h
# vs ~15 min for this script, and every gate that matters runs here:
# fmt+clippy+test and the release perf budgets up front, replay-clean
# and the version probe after the roll.
#
# Usage:
#   ./scripts/direct-deploy.sh <version>            # e.g. 2.9.14
#   SKIP_BUILD=1 ./scripts/direct-deploy.sh <ver>   # reuse last local image
#   SKIP_GATE=1  ./scripts/direct-deploy.sh <ver>   # emergency: skip the gate
#
# Steps:
#   0. gate: fmt + clippy + test + release perf gates. This used to be
#      described here as living "outside CI" and was in practice
#      whatever the person deploying remembered to run. It is now part
#      of the script, because the alternative is that it happens
#      sometimes.
#   1. buildx a linux/arm64 image locally (t01 + t02 are both arm64)
#   2. push ghcr.io/goliajp/mailrs:<version> (arm64-only; best-effort —
#      a push failure warns but never blocks the deploy)
#   3. prod (t02): save|ssh load, bump MAILRS_VERSION, compose up
#   (staging retired 2026-07-21 — prod is the only target)
#      (it ships the image + soak harness + kicks the 30-min soak)
#   4. verify prod: :3301 up, health version matches, AOF replay (clean)
#   5. prune target/: cargo never reclaims superseded artifacts, and a
#      deploy is the moment the churn just happened.
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

if [ "${SKIP_GATE:-0}" != 1 ]; then
    echo "==> [0/5] gate: fmt + clippy + test + perf"

    # testcontainers goes through bollard, which does not pick up the
    # credentials the Docker CLI has, so a cold pull fails with
    # "401 authentication required" and reads like an auth bug rather
    # than a missing image. Warm them first; both are no-ops once cached.
    docker pull -q postgres:11-alpine >/dev/null 2>&1 || true
    docker pull -q axllent/mailpit:latest >/dev/null 2>&1 || true

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    # --no-fail-fast: cargo stops at the first failing test binary, which
    # hides every red behind it. Three separate failures were found this
    # way on 2026-07-29, each only after the previous one was fixed.
    cargo test --workspace --no-fail-fast
    # Release-profile budgets, ~23s once target/release is warm; the
    # first run after a clean target/ pays for a full fat-LTO build.
    ./scripts/perf-gates.sh
else
    echo "!! [0/5] SKIP_GATE=1 — shipping unverified code to prod"
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
# Ship the compose file too. Without this a deploy silently keeps the
# host's old one, so any environment change (a new variable, a changed
# default) never reaches the containers while the version number and
# the image both look correct. That is exactly how MAILRS_DKIM_KEYS sat
# unread in .env while every domain signed with the wrong d= — see
# v2.10.0. A timestamped backup stays on the host for rollback.
ssh "$PROD" "cd /apps/mailrs && cp docker-compose.yml docker-compose.yml.bak-\$(date +%Y%m%d-%H%M%S)"
scp -q deploy/docker-compose.prod.yml "$PROD:/apps/mailrs/docker-compose.yml"
ssh "$PROD" "cd /apps/mailrs \
  && sed -i 's/^MAILRS_VERSION=.*/MAILRS_VERSION=$VERSION/' .env \
  && docker compose up -d --pull never --no-deps receiver fastcore webapi-fc fastcore-sender"

echo "==> [4/5] verify prod"
for i in $(seq 1 90); do
    # any 2xx-4xx means the router is up; only connection failures loop
    CODE=$(ssh "$PROD" "docker exec mailrs-fastcore curl -s -m3 -o /dev/null -w '%{http_code}' http://localhost:3301/healthz" 2>/dev/null || true)
    if printf '%s' "$CODE" | grep -qE '^[0-9]+$' && [ "$CODE" != "000" ]; then
        echo "    fastcore :3301 up (healthz=$CODE, attempt $i/90)"
        break
    fi
    if [ "$i" = 90 ]; then
        echo "!! fastcore :3301 never came up after 90 attempts — investigate"
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
echo "==> [5/5] prune target/"
# The gate above just rebuilt the workspace in two profiles, and cargo
# keeps every superseded artifact forever. Left alone this reached
# 425 GB / 832,531 files against a 16 GB working set. Age-based, so the
# build that just passed stays warm for the next deploy.
./scripts/prune-target.sh

echo "done: prod on $VERSION (ghcr pushed)"
