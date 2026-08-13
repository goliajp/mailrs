#!/usr/bin/env bash
# bench-api-e2e.sh — one comparable panel per storage backend.
#
# Three arms, one dataset, one ruler:
#
#   pg        monolith over PostgreSQL 18 (docker)
#   spg       monolith over spg-embedded (in-process SQL)
#   fastcore  fastcore (kevy embedded) + webapi + a kevy-server container,
#             which is the production topology — webapi keeps sessions and
#             side-state in the shared network store, so an arm without it
#             would be timing a request path production does not have
#
# Usage:
#   scripts/bench-api-e2e.sh pg
#   scripts/bench-api-e2e.sh fastcore
#   KEVY_IMAGE=ghcr.io/goliajp/kevy:5.1.0 scripts/bench-api-e2e.sh fastcore
#   scripts/bench-api-e2e.sh --compare        # 3-way table across saved runs
#
# Env:
#   ROUNDS=5   timed rounds (median across them, with sample deviation)
#   N=30       requests per endpoint per round
#   OUT=...    where per-arm results land (default bench-results/)
#
# Each run writes $OUT/<arm>.ndjson. `--compare` reduces all of them into
# one table and FAILS if the arms disagree about the dataset — see the
# fingerprint note below.
#
# On method, because it is the whole point of this script:
#
#   - The dataset is generated once by bench-api-seed.py and emitted as SQL
#     or NDJSON from the same rows, so the arms are fed the same work.
#   - Before any timing, each arm is fingerprinted: the first page of the
#     conversation list, as (thread_id, subject, message count) in order.
#     Order is included deliberately — score drift produces identical sets
#     in different orders, and that is what makes a paginated reader skip
#     or repeat a row.
#   - Percentiles are computed per round and the median across rounds is
#     reported with its deviation. A difference smaller than the deviation
#     is noise, not a result.
#   - Memory and CPU are sampled for EVERY process the arm runs, and the
#     sum is reported: the kevy arm runs a core and a web tier where the
#     monolith arms run one process, so a per-process comparison would
#     flatter the pair.
#
# Not in the panel yet: IMAP SELECT + FETCH. Both the monolith and fastcore
# serve IMAP, and it belongs here, but it needs a raw-socket client rather
# than curl and is tracked separately rather than half-wired.

set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"

OUT="${OUT:-bench-results}"
ROUNDS="${ROUNDS:-5}"
N="${N:-30}"
WARMUP="${WARMUP:-3}"
HOSTLABEL="${HOSTLABEL:-$(uname -s)-$(uname -m)}"

# ── compare mode ────────────────────────────────────────────────────────
if [ "${1:-}" = "--compare" ]; then
    exec python3 scripts/bench-api-compare.py "$OUT"
fi

ARM="${1:?usage: bench-api-e2e.sh pg|spg|fastcore   (or --compare)}"

PORT="${PORT:-3209}"          # the arm's public web API
CORE_PORT="${CORE_PORT:-3211}"  # fastcore's core-api
PG_PORT="${PG_PORT:-54329}"
KEVY_PORT="${KEVY_PORT:-63791}"
PG_CONTAINER="mailrs-bench-pg-$$"
KEVY_CONTAINER="mailrs-bench-kevy-$$"
SPG_IMAGE="${SPG_IMAGE:-goliakk/spg:latest}"
KEVY_IMAGE="${KEVY_IMAGE:-ghcr.io/goliajp/kevy:3.18.0}"
SECRET="bench-core-secret"
BASE="http://127.0.0.1:${PORT}"

# Every curl is bounded. A request that never returns has no latency to
# record, so an unbounded one turns a measurement into a hang — and a hang
# here looks exactly like a slow build, which is how one sat unnoticed for
# two hours.
CURL_T=(--connect-timeout 5 -m 30)

WORK="$(mktemp -d /tmp/mailrs-bench-api.XXXXXX)"
SAMPLES="$WORK/samples.ndjson"
PROC_NAMES=()
PROC_PIDS=()
SAMPLER_PID=""

cleanup() {
    # Reap the sampler quietly. Without the `wait`, the shell prints the
    # whole subshell body as a "Terminated" job notice into the middle of
    # the results.
    if [ -n "$SAMPLER_PID" ]; then
        { kill "$SAMPLER_PID" && wait "$SAMPLER_PID"; } 2>/dev/null || true
    fi
    # TERM, then escalate. A measured fastcore did not exit within 3s of
    # SIGTERM — its handler flushes kevy first — and a survivor holds the
    # ports the next arm needs, so the run after it fails for a reason that
    # has nothing to do with the run.
    for pid in "${PROC_PIDS[@]:-}"; do
        [ -n "$pid" ] && kill -TERM "$pid" 2>/dev/null || true
    done
    for _ in 1 2 3 4 5 6; do
        alive=0
        for pid in "${PROC_PIDS[@]:-}"; do
            [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && alive=1
        done
        [ "$alive" = 0 ] && break
        sleep 0.5
    done
    for pid in "${PROC_PIDS[@]:-}"; do
        [ -n "$pid" ] && kill -KILL "$pid" 2>/dev/null || true
    done
    docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
    docker rm -f "$KEVY_CONTAINER" >/dev/null 2>&1 || true
    rm -rf "$WORK" 2>/dev/null || true
}
trap cleanup EXIT

emit() { printf '%s\n' "$1" >> "$SAMPLES"; }

# ── resource sampler ────────────────────────────────────────────────────
# One record per process per second. `ps -o rss=,pcpu=` is portable across
# macOS and Linux; the arm's own disk footprint is read once at the end,
# when it has stopped growing.
start_sampler() {
    (
        # A missed sample is a gap in a chart; a sampler that exits on the
        # first `ps` miss (a process gone, a herestring empty) is a chart
        # that silently stops. `set -e` is inherited into this subshell, so
        # it comes off here deliberately.
        set +e
        while :; do
            for i in "${!PROC_PIDS[@]}"; do
                pid="${PROC_PIDS[$i]}"
                name="${PROC_NAMES[$i]}"
                rss="$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')"
                cpu="$(ps -o pcpu= -p "$pid" 2>/dev/null | tr -d ' ')"
                if [ -n "$rss" ] && [ -n "$cpu" ]; then
                    emit "{\"kind\":\"resource\",\"proc\":\"$name\",\"rss_kb\":$rss,\"cpu_pct\":$cpu}"
                fi
            done
            sleep 1
        done
    ) &
    SAMPLER_PID=$!
}

wait_http() {
    local url="$1" want="$2" tries="${3:-120}"
    for _ in $(seq 1 "$tries"); do
        if [ "$(curl --connect-timeout 2 -m 5 -s -o /dev/null -w '%{http_code}' "$url" 2>/dev/null)" = "$want" ]; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

echo "== seed: generating the dataset (deterministic) =="
python3 scripts/bench-api-seed.py > "$WORK/seed.sql"
python3 scripts/bench-api-seed.py --format ndjson > "$WORK/seed.ndjson"

# ── boot ────────────────────────────────────────────────────────────────
case "$ARM" in
  pg|spg)
    if [ "$ARM" = pg ]; then
        echo "== backend: postgres (pgvector/pgvector:pg18 on :$PG_PORT) =="
        docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
        docker run -d --name "$PG_CONTAINER" -p "$PG_PORT:5432" \
          -e POSTGRES_PASSWORD=bench -e POSTGRES_DB=mailrs_bench \
          pgvector/pgvector:pg18 >/dev/null
        # pg_isready passes during the throwaway initdb phase — probe the
        # actual database instead.
        until docker exec "$PG_CONTAINER" psql -U postgres -d mailrs_bench -qc "SELECT 1" >/dev/null 2>&1; do
            sleep 0.5
        done
        docker exec -i "$PG_CONTAINER" psql -q -U postgres -d mailrs_bench -v ON_ERROR_STOP=1 \
          < scripts/init-schema.sql
        echo "== seed: importing (~100 MB of INSERTs) =="
        docker exec -i "$PG_CONTAINER" psql -q -U postgres -d mailrs_bench -v ON_ERROR_STOP=1 \
          < "$WORK/seed.sql"
        DB_URL="postgres://postgres:bench@127.0.0.1:${PG_PORT}/mailrs_bench"
        FEATURES=""
        DISK_LABEL=""
    else
        echo "== backend: spg-embedded ($SPG_IMAGE) =="
        mkdir -p "$WORK/spg"
        docker run --rm -v "$WORK/spg:/work" -v "$ROOT/scripts:/scripts:ro" \
          --entrypoint spg "$SPG_IMAGE" \
          import --db /work/mailrs.spg --file /scripts/init-schema.sql
        docker run --rm -v "$WORK/spg:/work" -v "$WORK:/seed:ro" \
          --entrypoint spg "$SPG_IMAGE" \
          import --db /work/mailrs.spg --file /seed/seed.sql
        chmod -R u+rw "$WORK/spg" 2>/dev/null || true
        DB_URL="spg://$WORK/spg/mailrs.spg"
        FEATURES="--features spg"
        DISK_LABEL="spg catalog + WAL"
        DISK_DIR="$WORK/spg"
    fi

    echo "== build: mailrs-server --release ${FEATURES:-} =="
    # shellcheck disable=SC2086 — FEATURES is intentionally word-split
    cargo build --release -p mailrs-server $FEATURES 2>&1 | tail -1
    TARGET="$(cargo metadata --format-version 1 --no-deps \
      | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"

    mkdir -p "$WORK/maildir"
    env -i PATH="$PATH" HOME="$HOME" \
      MAILRS_HOSTNAME=localhost \
      MAILRS_MAILDIR="$WORK/maildir" \
      MAILRS_WEB_PORT="$PORT" \
      MAILRS_PG_URL="$DB_URL" \
      MAILRS_LOCAL_DOMAINS=bench.local \
      MAILRS_DNSBL_ENABLED=false \
      MAILRS_ANTISPAM_ENABLED=false \
      MAILRS_AI_ANALYSIS_ENABLED=false \
      MAILRS_SMTP_PORT=0 MAILRS_SUBMISSION_PORT=0 MAILRS_IMAP_PORT=0 \
      "$TARGET/release/mailrs-server" > "$WORK/server.log" 2>&1 &
    PROC_PIDS+=($!); PROC_NAMES+=("mailrs-server")
    ;;

  fastcore)
    echo "== backend: fastcore (kevy embedded) + webapi + $KEVY_IMAGE =="
    docker rm -f "$KEVY_CONTAINER" >/dev/null 2>&1 || true
    docker run -d --name "$KEVY_CONTAINER" -p "$KEVY_PORT:6379" "$KEVY_IMAGE" >/dev/null
    KEVY_URL="kevy://127.0.0.1:${KEVY_PORT}"

    echo "== build: fastcore + webapi --release =="
    cargo build --release -p mailrs-fastcore -p mailrs-webapi \
      --bin mailrs-fastcore --bin mailrs-fastcore-migrate --bin mailrs-webapi 2>&1 | tail -1
    TARGET="$(cargo metadata --format-version 1 --no-deps \
      | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"

    KEVY_DIR="$WORK/kevy-fastcore"
    mkdir -p "$KEVY_DIR"
    echo "== seed: replaying through deliver_message =="
    MAILRS_KEVY_DATA_DIR="$KEVY_DIR" \
      "$TARGET/release/mailrs-fastcore-migrate" < "$WORK/seed.ndjson" 2>&1 | tail -1

    env -i PATH="$PATH" HOME="$HOME" \
      MAILRS_KEVY_DATA_DIR="$KEVY_DIR" \
      MAILRS_FASTCORE_BIND="127.0.0.1:$CORE_PORT" \
      MAILRS_CORE_API_SECRET="$SECRET" \
      MAILRS_KEVY_URL="$KEVY_URL" \
      MAILRS_MAILDIR="$WORK/maildir" \
      "$TARGET/release/mailrs-fastcore" > "$WORK/fastcore.log" 2>&1 &
    PROC_PIDS+=($!); PROC_NAMES+=("mailrs-fastcore")

    wait_http "http://127.0.0.1:$CORE_PORT/v1/healthz" 200 120 || {
        echo "fastcore never came up:"; tail -20 "$WORK/fastcore.log"; exit 1; }

    env -i PATH="$PATH" HOME="$HOME" \
      MAILRS_CORE_RPC_BASE="http://127.0.0.1:$CORE_PORT" \
      MAILRS_CORE_API_SECRET="$SECRET" \
      MAILRS_WEB_BIND="127.0.0.1:$PORT" \
      MAILRS_KEVY_URL="$KEVY_URL" \
      MAILRS_AI_ANALYSIS_ENABLED=false \
      "$TARGET/release/mailrs-webapi" > "$WORK/webapi.log" 2>&1 &
    PROC_PIDS+=($!); PROC_NAMES+=("mailrs-webapi")

    DISK_LABEL="kevy data dir"
    DISK_DIR="$KEVY_DIR"
    ;;

  *) echo "unknown arm: $ARM (want pg|spg|fastcore)" >&2; exit 1 ;;
esac

wait_http "$BASE/api/health" 200 120 || {
    echo "web API never came up. logs:"; tail -20 "$WORK"/*.log; exit 1; }

echo "== login =="
TOKEN="$(curl "${CURL_T[@]}" -fsS -X POST "$BASE/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"address":"bench@bench.local","password":"bench-password"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['token'])")"
AUTH=(-H "Authorization: Bearer $TOKEN")

# ── fingerprint: is this arm holding the same dataset, in the same order? ─
echo "== fingerprint =="
FP_JSON="$(curl "${CURL_T[@]}" -fsS "${AUTH[@]}" "$BASE/api/conversations?limit=50")"
FP="$(printf '%s' "$FP_JSON" | python3 -c "
import hashlib, json, sys
rows = json.load(sys.stdin)
# Order-sensitive on purpose: identical membership in a different order is
# exactly the drift a paged reader turns into user-visible churn.
key = '|'.join(f\"{r.get('thread_id')}:{r.get('subject')}:{r.get('message_count', r.get('count'))}\" for r in rows)
print(f\"{len(rows)}:{hashlib.sha256(key.encode()).hexdigest()[:16]}\")
")"
echo "   first page: $FP"
THREAD_ID="$(printf '%s' "$FP_JSON" | python3 -c "import json,sys; print(json.load(sys.stdin)[0]['thread_id'])")"
echo "   probe thread: $THREAD_ID"

# ── the panel ───────────────────────────────────────────────────────────
# Endpoints every arm serves, except where marked. `unseen-count` exists
# only in webapi — the monolith's web module never grew it, so it is a
# kevy-arms-only row and the A-vs-B comparison is where it means something.
declare -a EP_NAME EP_URL EP_METHOD
add_ep() { EP_NAME+=("$1"); EP_URL+=("$2"); EP_METHOD+=("${3:-GET}"); }

add_ep "conversations?limit=50"   "$BASE/api/conversations?limit=50"
add_ep "conversations/{thread}"   "$BASE/api/conversations/$THREAD_ID"
add_ep "search?q=invoice"         "$BASE/api/conversations/search?q=invoice&limit=50"
add_ep "conversations/categories" "$BASE/api/conversations/categories"
add_ep "mail/stats"               "$BASE/api/mail/stats"
if [ "$ARM" = fastcore ]; then
    add_ep "conversations/unseen-count" "$BASE/api/conversations/unseen-count"
fi
# Reversible mutations: star then unstar, so every round starts from the
# same state and the numbers stay comparable round to round.
add_ep "thread/star"   "$BASE/api/conversations/$THREAD_ID/star"   POST
add_ep "thread/unstar" "$BASE/api/conversations/$THREAD_ID/unstar" POST
add_ep "thread/read"   "$BASE/api/conversations/$THREAD_ID/read"   POST

start_sampler

echo "== panel: $ROUNDS rounds x $N requests, warmup $WARMUP =="
for round in $(seq 1 "$ROUNDS"); do
    printf '   round %s/%s' "$round" "$ROUNDS"
    for i in "${!EP_NAME[@]}"; do
        name="${EP_NAME[$i]}"; url="${EP_URL[$i]}"; method="${EP_METHOD[$i]}"
        curlargs=("${CURL_T[@]}" -fsS -o /dev/null "${AUTH[@]}")
        [ "$method" = POST ] && curlargs+=(-X POST -H 'Content-Type: application/json' -d '{}')
        for _ in $(seq 1 "$WARMUP"); do curl "${curlargs[@]}" "$url" >/dev/null 2>&1 || true; done
        times="$(for _ in $(seq 1 "$N"); do
            curl "${curlargs[@]}" -w '%{time_total}\n' "$url" 2>/dev/null || echo 0
        done | python3 -c "
import sys
print(','.join(str(float(l.strip())*1000) for l in sys.stdin if l.strip()))
")"
        emit "{\"kind\":\"latency\",\"endpoint\":\"$name\",\"round\":$round,\"ms\":[$times]}"
        printf '.'
    done

    # Four parallel inbox reads — the round-22 starvation shape.
    #
    # `wait` with no arguments waits for EVERY background job, and this
    # script has one that never finishes: the resource sampler. That hung a
    # run for two hours with everything else healthy — the arm booted, the
    # API answered, and the script simply never came back. Wait on exactly
    # these four pids.
    start="$(python3 -c 'import time; print(time.time())')"
    cpids=()
    for _ in 1 2 3 4; do
        curl "${CURL_T[@]}" -fsS -o /dev/null "${AUTH[@]}" "$BASE/api/conversations?limit=50" &
        cpids+=($!)
    done
    wait "${cpids[@]}"
    wall="$(python3 -c "import time; print((time.time() - $start) * 1000)")"
    emit "{\"kind\":\"latency\",\"endpoint\":\"4-way concurrent inbox\",\"round\":$round,\"ms\":[$wall]}"
    printf ' done\n'
done

kill "$SAMPLER_PID" 2>/dev/null || true
SAMPLER_PID=""

# ── footprint ───────────────────────────────────────────────────────────
if [ -n "${DISK_DIR:-}" ] && [ -d "$DISK_DIR" ]; then
    emit "{\"kind\":\"disk\",\"label\":\"$DISK_LABEL\",\"kb\":$(du -sk "$DISK_DIR" | awk '{print $1}')}"
fi
if [ "$ARM" = pg ]; then
    kb="$(docker exec "$PG_CONTAINER" psql -U postgres -d mailrs_bench -tAc \
      "SELECT pg_database_size('mailrs_bench')/1024" 2>/dev/null || echo 0)"
    emit "{\"kind\":\"disk\",\"label\":\"postgres database\",\"kb\":${kb:-0}}"
fi
if [ "$ARM" = fastcore ]; then
    # The engine's own account of what it holds, which is the corroborating
    # witness for the disk figure above — a directory that grew and a key
    # count that did not would mean one of the two is measuring nothing.
    info="$(curl "${CURL_T[@]}" -fsS -X POST -H "Authorization: Bearer $SECRET" \
      "http://127.0.0.1:$CORE_PORT/v1/admin/maintenance:tier-info" 2>/dev/null || echo '{}')"
    printf '%s' "$info" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
for k in ('keys', 'used_memory', 'aof_bytes'):
    if k in d:
        print(json.dumps({'kind': 'engine', 'label': f'kevy {k}', 'value': d[k]}))
" >> "$SAMPLES" || true
    emit "{\"kind\":\"engine\",\"label\":\"kevy-server image\",\"value\":\"$KEVY_IMAGE\"}"
fi

emit "{\"kind\":\"meta\",\"arm\":\"$ARM\",\"rounds\":$ROUNDS,\"n\":$N,\"host\":\"$HOSTLABEL\",\"fingerprint\":\"$FP\",\"commit\":\"$(git rev-parse --short HEAD)\"}"

mkdir -p "$OUT"
cp "$SAMPLES" "$OUT/$ARM.ndjson"
python3 scripts/bench-stats.py < "$SAMPLES"
echo "saved: $OUT/$ARM.ndjson   (run '$0 --compare' once all arms are in)"
