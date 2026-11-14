#!/usr/bin/env bash
# interaction-latency.sh — measure the web client's interaction latency
# against the production shape, on this machine.
#
# Boots a kevy-server container, a mailrs-fastcore over a fresh kevy
# directory seeded with LATENCY_THREADS synthetic conversations (maildir
# bodies included), a mailrs-webapi, and `vite preview` of the production
# build; then runs web/tests/interaction-latency.spec.ts, which throttles
# the CPU 4x and the network to Fast 4G and reports P50/P95 per
# interaction.
#
# The binaries are not built here: release builds belong on a build host.
# Point BIN_DIR at a directory holding mailrs-fastcore,
# mailrs-fastcore-migrate, mailrs-parse-worker (fastcore starts it) and
# mailrs-webapi (default target/release).
#
# Exit status: 0 measured, 3 not measured (the machine was too busy, see
# the spec), anything else a failure. Not measured is not a pass.
#
# Env:
#   BIN_DIR           release binaries (default target/release)
#   LATENCY_THREADS   conversations to seed (default 30000)
#   KEVY_IMAGE        kevy-server image (default the one prod pins)
#   KEVY_BIN          run this kevy-server binary instead of the image, for a
#                     host without docker (same version as the image, please)
#   LATENCY_MAX_LOAD  passed to the spec, see there
#   SKIP_WEB_BUILD=1  reuse web/dist as it is
#   PW_CONFIG         run another Playwright config against the same stack,
#                     e.g. playwright.stack.config.ts (layout and live
#                     updates); its exit status is this script's
#   CORE=pg           serve the web from the SQL core instead: a PostgreSQL
#                     container and mailrs-pg-core come up beside fastcore,
#                     mailrs-core-sync copies the seeded mail across, and
#                     webapi talks to the SQL core. BIN_DIR must then hold
#                     mailrs-pg-core and mailrs-core-sync too.
#   PG_IMAGE          PostgreSQL image for CORE=pg (default the one the
#                     pg-core compose file pins)

set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"

BIN_DIR="${BIN_DIR:-$ROOT/target/release}"
THREADS="${LATENCY_THREADS:-30000}"
KEVY_IMAGE="${KEVY_IMAGE:-ghcr.io/goliajp/kevy:6.4.0}"
KEVY_PORT="${KEVY_PORT:-6048}"
CORE_PORT="${CORE_PORT:-6039}"
WEB_PORT="${WEB_PORT:-6040}"
PREVIEW_PORT="${PREVIEW_PORT:-6041}"
PG_PORT="${PG_PORT:-6050}"
PG_CORE_PORT="${PG_CORE_PORT:-6051}"
PG_IMAGE="${PG_IMAGE:-pgvector/pgvector:0.8.2-pg18-trixie}"
PG_CONTAINER="mailrs-latency-pg-$$"
CORE="${CORE:-fastcore}"
SECRET="latency-core-secret"
KEVY_CONTAINER="mailrs-latency-kevy-$$"

PORTS=("$KEVY_PORT" "$CORE_PORT" "$WEB_PORT" "$PREVIEW_PORT")
BINS=(mailrs-fastcore mailrs-fastcore-migrate mailrs-parse-worker mailrs-webapi)
case "$CORE" in
    fastcore) ;;
    pg)
        PORTS+=("$PG_PORT" "$PG_CORE_PORT")
        BINS+=(mailrs-pg-core mailrs-core-sync)
        ;;
    *) echo "CORE=$CORE: fastcore or pg" >&2; exit 1 ;;
esac
for port in "${PORTS[@]}"; do
    if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
        echo "port $port is already in use:" >&2
        lsof -nP -iTCP:"$port" -sTCP:LISTEN >&2
        exit 1
    fi
done
for bin in "${BINS[@]}"; do
    [ -x "$BIN_DIR/$bin" ] || { echo "missing $BIN_DIR/$bin" >&2; exit 1; }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/mailrs-latency.XXXXXX")"
PIDS=()

cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill -TERM "$pid" 2>/dev/null || true
    done
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        alive=0
        for pid in "${PIDS[@]:-}"; do
            [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && alive=1
        done
        [ "$alive" = 0 ] && break
        sleep 0.5
    done
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill -KILL "$pid" 2>/dev/null || true
    done
    if [ -z "${KEVY_BIN:-}" ]; then
        docker rm -f "$KEVY_CONTAINER" >/dev/null 2>&1 || true
    fi
    if [ "$CORE" = pg ]; then
        docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
    fi
    # the servers' own request timings are half of any decomposition
    mkdir -p "$ROOT/web/perf-results/latency-logs"
    cp "$WORK"/*.log "$ROOT/web/perf-results/latency-logs/" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

wait_http() {
    local url="$1" tries="${2:-120}"
    for _ in $(seq 1 "$tries"); do
        [ "$(curl --connect-timeout 2 -m 5 -s -o /dev/null -w '%{http_code}' "$url")" = 200 ] && return 0
        sleep 0.5
    done
    return 1
}

if [ -n "${KEVY_BIN:-}" ]; then
    echo "== kevy-server: $KEVY_BIN on :$KEVY_PORT =="
    mkdir -p "$WORK/kevy-server"
    env -i PATH="$PATH" HOME="$HOME" KEVY_BIND=127.0.0.1 KEVY_PORT="$KEVY_PORT" \
        KEVY_DIR="$WORK/kevy-server" KEVY_AOF=1 \
        "$KEVY_BIN" --config "$ROOT/deploy/kevy.toml" > "$WORK/kevy-server.log" 2>&1 &
    PIDS+=($!)
else
    echo "== kevy-server: $KEVY_IMAGE on :$KEVY_PORT =="
    # prod's config: the event feed the web socket reads is off without it
    docker run -d --name "$KEVY_CONTAINER" -p "127.0.0.1:$KEVY_PORT:6379" \
        -v "$ROOT/deploy/kevy.toml:/etc/kevy/kevy.toml:ro" "$KEVY_IMAGE" >/dev/null
fi
KEVY_URL="kevy://127.0.0.1:$KEVY_PORT"
for _ in $(seq 1 60); do
    nc -z 127.0.0.1 "$KEVY_PORT" 2>/dev/null && break
    sleep 0.5
done

echo "== seed: $THREADS conversations =="
mkdir -p "$WORK/kevy-fastcore" "$WORK/maildir"
python3 scripts/latency-seed.py --maildir "$WORK/maildir" --threads "$THREADS" > "$WORK/seed.ndjson"
MAILRS_KEVY_DATA_DIR="$WORK/kevy-fastcore" "$BIN_DIR/mailrs-fastcore-migrate" \
    < "$WORK/seed.ndjson" 2>&1 | tail -1

echo "== fastcore on :$CORE_PORT =="
env -i PATH="$PATH" HOME="$HOME" \
    MAILRS_KEVY_DATA_DIR="$WORK/kevy-fastcore" \
    MAILRS_FASTCORE_BIND="127.0.0.1:$CORE_PORT" \
    MAILRS_CORE_API_SECRET="$SECRET" \
    MAILRS_KEVY_URL="$KEVY_URL" \
    MAILRS_MAILDIR="$WORK/maildir" \
    MAILRS_HOSTNAME=localhost \
    MAILRS_LOCAL_DOMAINS=latency.local \
    MAILRS_IMAP_BIND=off MAILRS_IMAPS_BIND=off MAILRS_POP3_BIND=off \
    MAILRS_POP3S_BIND=off MAILRS_MANAGESIEVE_BIND=off \
    "$BIN_DIR/mailrs-fastcore" > "$WORK/fastcore.log" 2>&1 &
PIDS+=($!)
wait_http "http://127.0.0.1:$CORE_PORT/v1/healthz" 240 || {
    echo "fastcore never came up:" >&2; tail -20 "$WORK/fastcore.log" >&2; exit 1; }

WEB_CORE_PORT="$CORE_PORT"
if [ "$CORE" = pg ]; then
    echo "== PostgreSQL: $PG_IMAGE on :$PG_PORT =="
    docker run -d --name "$PG_CONTAINER" -p "127.0.0.1:$PG_PORT:5432" \
        -e POSTGRES_USER=mailrs -e POSTGRES_PASSWORD=latency -e POSTGRES_DB=mailrs \
        "$PG_IMAGE" >/dev/null
    for _ in $(seq 1 120); do
        docker exec "$PG_CONTAINER" pg_isready -U mailrs -d mailrs >/dev/null 2>&1 && break
        sleep 0.5
    done

    echo "== pg-core on :$PG_CORE_PORT =="
    mkdir -p "$WORK/kevy-pg-core"
    env -i PATH="$PATH" HOME="$HOME" \
        MAILRS_PG_URL="postgres://mailrs:latency@127.0.0.1:$PG_PORT/mailrs" \
        MAILRS_CORE_RPC_ADDR="127.0.0.1:$PG_CORE_PORT" \
        MAILRS_CORE_API_SECRET="$SECRET" \
        MAILRS_KEVY_DATA_DIR="$WORK/kevy-pg-core" \
        MAILRS_KEVY_URL="$KEVY_URL" \
        MAILRS_MAILDIR="$WORK/maildir" \
        MAILRS_HOSTNAME=localhost \
        MAILRS_LOCAL_DOMAINS=latency.local \
        MAILRS_IMAP_BIND=off MAILRS_IMAPS_BIND=off MAILRS_POP3_BIND=off \
        MAILRS_POP3S_BIND=off MAILRS_MANAGESIEVE_BIND=off \
        "$BIN_DIR/mailrs-pg-core" > "$WORK/pg-core.log" 2>&1 &
    PIDS+=($!)
    wait_http "http://127.0.0.1:$PG_CORE_PORT/v1/healthz" 240 || {
        echo "pg-core never came up:" >&2; tail -20 "$WORK/pg-core.log" >&2; exit 1; }

    echo "== core-sync: fastcore -> pg-core =="
    env -i PATH="$PATH" HOME="$HOME" MAILRS_CORE_API_SECRET="$SECRET" \
        "$BIN_DIR/mailrs-core-sync" --from "http://127.0.0.1:$CORE_PORT" \
        --to "http://127.0.0.1:$PG_CORE_PORT" > "$WORK/core-sync.log" 2>&1 || {
        echo "core-sync failed:" >&2; tail -20 "$WORK/core-sync.log" >&2; exit 1; }
    tail -3 "$WORK/core-sync.log"
    WEB_CORE_PORT="$PG_CORE_PORT"
fi

echo "== webapi on :$WEB_PORT -> core :$WEB_CORE_PORT =="
env -i PATH="$PATH" HOME="$HOME" \
    MAILRS_CORE_RPC_BASE="http://127.0.0.1:$WEB_CORE_PORT" \
    MAILRS_CORE_API_SECRET="$SECRET" \
    MAILRS_WEB_BIND="127.0.0.1:$WEB_PORT" \
    MAILRS_KEVY_URL="$KEVY_URL" \
    MAILRS_MAILDIR="$WORK/maildir" \
    MAILRS_HOSTNAME=localhost \
    MAILRS_LOCAL_DOMAINS=latency.local \
    MAILRS_AI_ANALYSIS_ENABLED=false \
    MAILRS_WEB_ORIGINS="http://localhost:$PREVIEW_PORT" \
    "$BIN_DIR/mailrs-webapi" > "$WORK/webapi.log" 2>&1 &
PIDS+=($!)
wait_http "http://127.0.0.1:$WEB_PORT/api/health" 120 || {
    echo "webapi never came up:" >&2; tail -20 "$WORK/webapi.log" >&2; exit 1; }

cd web
if [ "${SKIP_WEB_BUILD:-}" != 1 ]; then
    echo "== web: production build =="
    bunx vite build > "$WORK/vite-build.log" 2>&1 || { tail -20 "$WORK/vite-build.log" >&2; exit 1; }
fi

echo "== vite preview on :$PREVIEW_PORT -> webapi :$WEB_PORT =="
MAILRS_API_ORIGIN="http://127.0.0.1:$WEB_PORT" \
    bunx vite preview --port "$PREVIEW_PORT" --strictPort > "$WORK/preview.log" 2>&1 &
PIDS+=($!)
wait_http "http://localhost:$PREVIEW_PORT/" 60 || {
    echo "vite preview never came up:" >&2; tail -20 "$WORK/preview.log" >&2; exit 1; }

echo "== measuring =="
set +e
LATENCY_BASE="http://localhost:$PREVIEW_PORT" \
LATENCY_USER="bench@latency.local" LATENCY_PASS="bench-password" \
LATENCY_THREADS="$THREADS" \
    bunx playwright test -c "${PW_CONFIG:-playwright.latency.config.ts}" "$@"
rc=$?
set -e

RESULT="perf-results/interaction-latency.json"
if [ "$rc" = 0 ] && [ -f "$RESULT" ] && grep -q '"verdict": "not measured"' "$RESULT"; then
    echo "!! 未测 / not measured: the machine was too busy, see $RESULT" >&2
    exit 3
fi
if [ "$rc" != 0 ]; then
    echo "-- server logs (tail) --" >&2
    tail -5 "$WORK"/*.log >&2
fi
exit "$rc"
