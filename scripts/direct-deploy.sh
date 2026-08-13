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
#   WEB_ONLY=1   ./scripts/direct-deploy.sh <ver>   # web-only roll
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

# The inverse of SKIP_WEB, and the reason it had to exist: the browser
# bundle changes far more often than the binary, and without this the
# only way to ship a frontend fix was a twenty-minute image rebuild that
# produced a byte-identical binary. On 2026-08-05 prod was serving a
# bundle five commits old — including the fix for the bug being reported
# — because that rebuild is enough friction to put the deploy off.
#
# It skips the Rust gate too. Nothing Rust ships, so `cargo test` here
# would only be verifying that the commit prod is already running still
# passes. `bun run check` + `bun run test` still run, inside step 5.
if [ "${WEB_ONLY:-0}" = 1 ]; then
    echo "==> WEB_ONLY=1 — steps 0-4 and 6 skipped; prod keeps the binary it is running"
    if [ "${SKIP_WEB:-0}" = 1 ]; then
        echo "!! WEB_ONLY=1 and SKIP_WEB=1 together ship nothing"
        exit 1
    fi
    # Not SKIP_GATE: that one prints "shipping unverified code to prod",
    # which would be false here — step 5 still runs check + test.
    WEB_ONLY_SKIPS_RUST_GATE=1
fi

if [ "${SKIP_GATE:-0}" != 1 ] && [ "${WEB_ONLY_SKIPS_RUST_GATE:-0}" != 1 ]; then
    echo "==> [0/6] gate: parity + fmt + clippy + test + perf"

    # webapi and fastcore are two processes talking over the core RPC
    # contract, and the client spells every path again in a `format!` —
    # a typo on either side is a 404 nothing else catches.
    #
    # This replaced check-rest-parity.sh, which compared production
    # against `crates/server`. The monolith is not coming back, so that
    # gate had become 51 allow-list lines of paperwork for routes that
    # were never going to exist twice.
    #
    # Neither check compiles anything, so both run first — a break is
    # cheaper to learn about before a ten-minute test run than after.
    ./scripts/check-mcp-parity.sh
    ./scripts/check-core-contract.sh
    # The two cores must serve one contract: pg-core ⊆ fastcore hard, and
    # the reverse as a ratchet (that set is the pg-core reactivation debt).
    # Replaces core_rpc/tests/parity.rs, which named both router files by
    # path — both had moved, one to a path that never existed — and only
    # compiled under a feature nothing built, so none of it ever fired.
    ./scripts/check-core-parity.sh

    # A crate in this repo depended on by version alone resolves to the
    # published copy, so both end up in the binary and edits to the local
    # one do nothing. 23 crates were duplicated that way until
    # 2026-08-03, two at different versions. Costs nothing to check.
    ./scripts/check-workspace-deps.sh

    # Three checks for one failure mode: a capability that exists on
    # one side of the wire and nowhere on the other, with nothing
    # saying so. Each was written on 2026-08-10 after finding its own
    # instance by reading, and each found more the moment it ran.
    #
    #   dead-routes   — a registered route no client calls. Found
    #                   `POST /api/scheduled/{id}/cancel` with no
    #                   caller anywhere, on the day iOS learned to
    #                   *create* a scheduled send.
    #   inert-fields  — a field written to a thread hash that no reader
    #                   parses. That was `snoozed_until`: snoozing did
    #                   nothing at all, on both clients, for months.
    #   outbound-keys — a queue key spelled instead of imported. The
    #                   sender's due-sweep had drifted to a key nothing
    #                   writes, so no scheduled message ever left.
    ./scripts/check-dead-routes.sh
    ./scripts/check-inert-fields.sh
    ./scripts/check-outbound-keys.sh

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
    # The dormant pg/spg lane, which nothing above reaches: features are off
    # by default, so `--workspace --all-targets` never sees the code behind
    # `spg` or `core-rpc`, and test.yml — the only thing that did build them
    # — runs on master/release/hotfix, whose tip predates develop by weeks.
    #
    # It had been broken since the 2026-08-02 file split, in 23 places. All
    # of them one root cause: that commit moved these files a directory
    # deeper and nothing that referenced position followed — relative
    # `include_str!` paths, `super::*` globs, and a re-export widened past
    # its own visibility. Plus a `cfg` with two comma-separated predicates,
    # which is malformed, so that file had never compiled at all.
    #
    # `--all-targets` matters and both axes matter: the lane's two test files
    # sit on opposite sides of the `spg` switch (the in-memory pg-core suite
    # needs it, the real-Postgres bidirectional sync test needs it off), so
    # a single invocation type-checks exactly one of them. Test-only rot is
    # the kind this lane had.
    # ~19s cold for the spg dependency tree, seconds warm.
    cargo check -p mailrs-server --features core-rpc,spg --all-targets
    cargo check -p mailrs-server --features core-rpc --all-targets
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

if [ "${WEB_ONLY:-0}" = 1 ]; then
    echo "==> [1-4/6] skipped (WEB_ONLY)"
elif [ "${SKIP_BUILD:-0}" != 1 ]; then
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

if [ "${WEB_ONLY:-0}" != 1 ]; then
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
# v2.10.0.
#
# The old line here kept a timestamped copy on every deploy, and by
# 2026-08-12 that was 126 files nobody had ever opened. A backup taken
# unconditionally is not a safety net: this file is derived from
# `deploy/docker-compose.prod.yml`, so every one of those copies was a
# worse duplicate of something git already had, and the one case that
# genuinely could not be recovered — someone editing the host copy by
# hand — was buried in the churn.
#
# So the copy is conditional, and it converges: the host file is
# compared against the exact bytes the last deploy wrote, kept beside
# it. Equal means nothing is at risk and nothing is written. Different
# means the host was edited by hand and that edit is about to be
# overwritten — worth a timestamped copy *and* saying so, because the
# person who made it is not necessarily the person deploying.
#
# `.prev` is a single overwritten file for "put it back right now";
# anything older is a git checkout away.
ssh "$PROD" 'cd /apps/mailrs || exit 1
if [ -f docker-compose.yml.deployed ] && ! cmp -s docker-compose.yml docker-compose.yml.deployed; then
    keep="docker-compose.yml.bak-$(date +%Y%m%d-%H%M%S)"
    cp docker-compose.yml "$keep"
    echo "    !! host compose was edited by hand since the last deploy — kept $keep"
    diff docker-compose.yml.deployed docker-compose.yml | head -20 | sed "s/^/       /"
fi
cp docker-compose.yml docker-compose.yml.prev 2>/dev/null || true
# Bound the hand-edit copies too. They should be rare; a runaway here
# would just be the old problem wearing a condition.
ls -t docker-compose.yml.bak-20* 2>/dev/null | tail -n +11 | xargs -r rm -f'
scp -q deploy/docker-compose.prod.yml "$PROD:/apps/mailrs/docker-compose.yml"
# Record what we wrote, so the next deploy can tell our own bytes from
# someone else's edit.
ssh "$PROD" "cd /apps/mailrs && cp docker-compose.yml docker-compose.yml.deployed"
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
fi

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

if [ "${WEB_ONLY:-0}" = 1 ]; then
    echo "==> [6/6] skipped (WEB_ONLY) — no cargo build ran, nothing to reclaim"
    echo "done: prod web now serving direct-$VERSION; binary untouched"
    exit 0
fi

echo "==> [6/6] prune target/"
# The gate above just rebuilt the workspace in two profiles, and cargo
# keeps every superseded artifact forever. Left alone this reached
# 425 GB / 832,531 files against a 16 GB working set. Age-based, so the
# build that just passed stays warm for the next deploy.
./scripts/prune-target.sh

echo "done: prod on $VERSION (ghcr pushed)"
