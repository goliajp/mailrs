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
#   SKIP_WEB=1   ./scripts/direct-deploy.sh <ver>   # backend-only roll
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
#   5. web: build + rsync into the webapi-fc bind mount, then verify the
#      container actually serves the new bundle. Added 2026-07-30 — until
#      then this script shipped only the containers, so every frontend
#      change since 2026-07-08 reached prod with the old bundle serving.
#   6. prune target/: cargo never reclaims superseded artifacts, and a
#      deploy is the moment the churn just happened.
set -euo pipefail

VERSION="${1:?usage: direct-deploy.sh <version>}"
TAG="mailrs:staging-local"
GHCR="ghcr.io/goliajp/mailrs:$VERSION"
PROD="root@t02.golia.jp"
cd "$(dirname "$0")/.."

assert_clean_tree() {
    if [ -n "$(git status --porcelain)" ]; then
        echo "!! working tree dirty — commit first (deploys must be reproducible)"
        echo "!! $(git status --porcelain | wc -l | tr -d ' ') path(s):"
        git status --porcelain | head -10
        exit 1
    fi
}

HEAD_AT_START="$(git rev-parse HEAD)"
assert_clean_tree

if [ "${SKIP_GATE:-0}" != 1 ]; then
    echo "==> [0/6] gate: parity + fmt + clippy + test + perf"

    # Two lanes serve one client, so a route or MCP tool on one and not the
    # other is a feature that works or 405s depending on which is deployed.
    # Neither check compiles anything, so both run first — a parity break is
    # cheaper to learn about before a ten-minute test run than after.
    ./scripts/check-mcp-parity.sh
    ./scripts/check-rest-parity.sh

    # The 500-line limit, as a ratchet: a new file over it fails, and a
    # file already on the baseline may only shrink. 51 files were over it
    # with no gate at all, because the copy of `file-size.md` this repo
    # carried until 2026-08-02 listed *torajs*'s debt table — so mailrs's
    # own overruns had never been written down.
    ./scripts/check-file-size.sh

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
    echo "!! [0/6] SKIP_GATE=1 — shipping unverified code to prod"
fi

# The gate takes minutes and `docker buildx build .` sends the working
# directory, not the commit — so a tree edited while the gate ran ships
# code the gate never saw and no commit records. Checking once at the
# top does not cover that window; this one caught nothing on 2026-07-30
# only because the deploy was killed by hand.
assert_clean_tree
if [ "$(git rev-parse HEAD)" != "$HEAD_AT_START" ]; then
    echo "!! HEAD moved during the gate ($HEAD_AT_START -> $(git rev-parse HEAD))"
    echo "!! the gate verified a different commit than this would ship"
    exit 1
fi

if [ "${SKIP_BUILD:-0}" != 1 ]; then
    echo "==> [1/6] local arm64 build ($VERSION)"
    docker buildx build \
        --platform linux/arm64 \
        --build-arg VERSION="$VERSION" \
        --build-arg CACHE_BUST="direct-$VERSION" \
        -t "$TAG" \
        --load \
        .
else
    echo "==> [1/6] SKIP_BUILD=1 — reusing local $TAG"
fi

echo "==> [2/6] push $GHCR (best-effort)"
docker tag "$TAG" "$GHCR"
if ! docker push "$GHCR"; then
    echo "!! ghcr push failed — continuing with the deploy (image still ships via save|load)"
fi

echo "==> [3/6] prod: save | ssh load + compose up"
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

echo "==> [4/6] verify prod"
for i in $(seq 1 90); do
    # `/v1/healthz`, and 200 specifically. This probed `/healthz` — which
    # does not exist and answers 404 — and then accepted any non-000 code
    # as success, so what it really tested was "something is listening".
    # That is enough to catch a crash-looping process, but it reports a
    # broken core as healthy, and it printed `healthz=404` on every deploy
    # for long enough that the number stopped being read.
    CODE=$(ssh "$PROD" "docker exec mailrs-fastcore curl -s -m3 -o /dev/null -w '%{http_code}' http://localhost:3301/v1/healthz" 2>/dev/null || true)
    if [ "$CODE" = 200 ]; then
        echo "    fastcore :3301 healthy (attempt $i/90)"
        break
    fi
    if [ "$i" = 90 ]; then
        echo "!! fastcore /v1/healthz never returned 200 after 90 attempts (last: '$CODE') — investigate"
    fi
    sleep 2
done
# webapi-fc's own router is built at process start, so a bad route takes
# it into a restart loop while every fastcore check above still passes —
# that is exactly what 2.19.0 did. Retry, then fail: this used to print
# the version and continue regardless, so a deploy that left the REST API
# and the web UI down still ended with "done". The web step happened to
# catch it, which means `SKIP_WEB=1` would not have.
GOT_VERSION=""
for i in $(seq 1 45); do
    GOT_VERSION=$(ssh "$PROD" "curl -s -m5 localhost:3103/api/health" 2>/dev/null | grep -o "\"version\":\"$VERSION\"" || true)
    if [ -n "$GOT_VERSION" ]; then
        echo "    webapi-fc healthy on $VERSION (attempt $i/45)"
        break
    fi
    sleep 2
done
if [ -z "$GOT_VERSION" ]; then
    echo "!! webapi-fc never reported $VERSION on :3103 — it may be restart-looping"
    echo "!! docker logs mailrs-webapi-fc | tail -30   (a route-table panic prints here)"
    ssh "$PROD" "docker ps --format '{{.Names}}\t{{.Status}}' | grep webapi" || true
    exit 1
fi
# The version gate answers "is the new binary running" and cannot answer
# "does it still serve the endpoints". Three AI routes and three admin verbs
# were missing in production while every check above read green.
./scripts/post-deploy-probe.sh

REPLAY=$(ssh "$PROD" "docker logs mailrs-fastcore 2>&1 | grep -iE 'kevy: AOF .* replayed' | tail -1" || true)
echo "    replay: $REPLAY"
case "$REPLAY" in
    *"(clean)"*) echo "    replay clean ✓" ;;
    *) echo "!! replay line is NOT clean — investigate before walking away (AOF black-hole SOP)" ;;
esac
echo "==> [5/6] web: build + rsync"
# Until 2026-07-30 this script shipped only the four Rust containers, so a
# release whose value was in the browser reached prod with the old bundle
# still being served: the backend had the endpoints, the UI had no way to
# call them, and both the image tag and the health version read correct.
# Every frontend change since 2026-07-08 sat undeployed for that reason.
#
# The web-v* CI lane still exists and still owns the tagged path; this is
# the same three steps it runs, locally. `/apps/mailrs/web` is
# bind-mounted into webapi-fc as /opt/mailrs/web:ro and ServeDir reads
# from disk per request, so no container restarts.
if [ "${SKIP_WEB:-0}" != 1 ]; then
    (
        cd web
        bun install --frozen-lockfile
        bun run check
        bun run test -- --run
        # Baked into the bundle via vite.config.ts's `__WEB_VERSION__` and
        # shown by the StatusBar. Prefixed so a directly-deployed bundle is
        # distinguishable from a web-v* one at a glance.
        WEB_VERSION="direct-$VERSION" bunx --bun tsc -b
        WEB_VERSION="direct-$VERSION" bunx --bun vite build
    )
    # No `--delete`. Asset filenames carry a content hash, and a browser tab
    # opened before a deploy still asks for the chunk names it was built
    # against. Deleting them breaks every live tab on the first lazy import:
    # `markdown-preview` is its own chunk, so pressing Preview in a reply
    # after a deploy produced "Something went wrong" — four deploys in one
    # hour on 2026-07-30, each one breaking the tab again.
    #
    # Old chunks are small and immutable; the prune below keeps the
    # directory from growing without bound.
    rsync -az -e ssh web/dist/ "$PROD:/apps/mailrs/web/"
    # Keep index.html exact: it is the one unhashed file, and a stale copy
    # would serve the previous build's chunk graph to new visitors.
    rsync -az -e ssh web/dist/index.html "$PROD:/apps/mailrs/web/index.html"
    # Retire assets untouched for 30 days. Long enough that no realistic tab
    # is still holding one, short enough that the directory stays small.
    #
    # The count is compared, not just printed. A `--delete` creeping back
    # into the rsync above, or a prune that suddenly matches everything,
    # breaks every tab opened before the deploy on its first lazy import —
    # `markdown-preview` is its own chunk, so pressing Preview in a reply
    # produced "Something went wrong" four times in one hour on 2026-07-30.
    # A printed number nobody compares would not have said so.
    BEFORE_ASSETS=$(ssh -n "$PROD" "ls -1 /apps/mailrs/web/assets 2>/dev/null | wc -l | tr -d ' '" || echo 0)
    ssh -n "$PROD" "find /apps/mailrs/web/assets -type f -mtime +30 -delete 2>/dev/null; true"
    AFTER_ASSETS=$(ssh -n "$PROD" "ls -1 /apps/mailrs/web/assets | wc -l | tr -d ' '" || echo 0)
    echo "    assets kept: $AFTER_ASSETS (was $BEFORE_ASSETS before the prune)"
    if [ "$AFTER_ASSETS" -lt "$BEFORE_ASSETS" ]; then
        DROPPED=$((BEFORE_ASSETS - AFTER_ASSETS))
        echo "!! $DROPPED asset(s) disappeared. The 30-day prune retires files"
        echo "!! that old, so a drop right after a deploy that just added"
        echo "!! chunks means something deleted live ones — check for"
        echo "!! --delete on the rsync. Tabs open before this deploy will"
        echo "!! break on their next lazy import."
        exit 1
    fi

    # Ask the container to serve the hash vite just emitted. A stale bind
    # mount or a wrong rsync path makes ServeDir fall through to the SPA
    # index and answer text/html — which looks like a working site while
    # serving the previous bundle.
    BUNDLE=$(grep -oE 'index-[A-Za-z0-9_-]+\.js' web/dist/index.html | head -1)
    CT=$(ssh "$PROD" "curl -sIo /dev/null -w '%{content_type}' 'http://localhost:3103/assets/$BUNDLE'" || true)
    echo "    bundle $BUNDLE -> $CT"
    case "$CT" in
        application/javascript*|text/javascript*) echo "    web bundle served ✓" ;;
        *) echo "!! web bundle NOT served (got '$CT') — the browser is still on the old build"; exit 1 ;;
    esac
else
    echo "!! [5/6] SKIP_WEB=1 — prod keeps the bundle it already has"
fi

echo "==> [6/6] prune target/"
# The gate above just rebuilt the workspace in two profiles, and cargo
# keeps every superseded artifact forever. Left alone this reached
# 425 GB / 832,531 files against a 16 GB working set. Age-based, so the
# build that just passed stays warm for the next deploy.
./scripts/prune-target.sh

echo "done: prod on $VERSION (ghcr pushed)"
