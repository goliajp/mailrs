#!/usr/bin/env bash
# e2e web-API perf baseline — runs the real mailrs-server (release build)
# against a seeded round-22-shape dataset (24 304 messages / 23 508
# threads, scripts/bench-api-seed.py) and times the endpoints the web UI
# actually hits. Purpose: a PG-era reference column so the SPG cutover
# can be compared apples-to-apples on the same panel (PERFORMANCE.md).
#
# Usage:
#   scripts/bench-api-e2e.sh pg            # docker pgvector backend
#   scripts/bench-api-e2e.sh spg           # spg-embedded backend
#   N=50 scripts/bench-api-e2e.sh pg       # requests per endpoint (default 30)
#
# Output: p50/p95/p99 ms per endpoint + a 4-way concurrent inbox wall time.

set -euo pipefail

BACKEND="${1:?usage: bench-api-e2e.sh pg|spg}"
N="${N:-30}"
WARMUP="${WARMUP:-3}"
PORT="${PORT:-3209}"
PG_CONTAINER="mailrs-bench-pg-$$"   # per-run name: parallel runs must not reap each other
PG_PORT="${PG_PORT:-54329}"
SPG_IMAGE="${SPG_IMAGE:-goliakk/spg:latest}"
BASE="http://127.0.0.1:${PORT}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d /tmp/mailrs-bench-api.XXXXXX)"
SERVER_PID=""
cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  # only reap our own container (pg mode creates it; spg mode never does)
  [ "$BACKEND" = pg ] && docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "== seed: generating dataset (deterministic) =="
python3 scripts/bench-api-seed.py > "$WORK/seed.sql"

case "$BACKEND" in
  pg)
    echo "== backend: postgres (pgvector/pgvector:pg18 on :$PG_PORT) =="
    docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
    docker run -d --name "$PG_CONTAINER" -p "$PG_PORT:5432" \
      -e POSTGRES_PASSWORD=bench -e POSTGRES_DB=mailrs_bench \
      pgvector/pgvector:pg18 >/dev/null
    # pg_isready can pass during the throwaway initdb phase — probe the
    # actual database instead
    until docker exec "$PG_CONTAINER" psql -U postgres -d mailrs_bench -qc "SELECT 1" >/dev/null 2>&1; do
      sleep 0.5
    done
    docker exec -i "$PG_CONTAINER" psql -q -U postgres -d mailrs_bench -v ON_ERROR_STOP=1 \
      < scripts/init-schema.sql
    echo "== seed: importing (this is ~100 MB of INSERTs) =="
    docker exec -i "$PG_CONTAINER" psql -q -U postgres -d mailrs_bench -v ON_ERROR_STOP=1 \
      < "$WORK/seed.sql"
    PG_URL="postgres://postgres:bench@127.0.0.1:${PG_PORT}/mailrs_bench"
    FEATURES=""
    ;;
  spg)
    echo "== backend: spg-embedded ($SPG_IMAGE) =="
    mkdir -p "$WORK/spg"
    docker run --rm -v "$WORK/spg:/work" -v "$ROOT/scripts:/scripts:ro" \
      --entrypoint spg "$SPG_IMAGE" \
      import --db /work/mailrs.spg --file /scripts/init-schema.sql
    docker run --rm -v "$WORK/spg:/work" -v "$WORK:/seed:ro" \
      --entrypoint spg "$SPG_IMAGE" \
      import --db /work/mailrs.spg --file /seed/seed.sql
    # container writes as its own uid; make readable for the host server
    chmod -R u+rw "$WORK/spg" 2>/dev/null || true
    PG_URL="spg://$WORK/spg/mailrs.spg"
    FEATURES="--features spg"
    ;;
  *) echo "unknown backend: $BACKEND" >&2; exit 1 ;;
esac

echo "== build: mailrs-server --release ${FEATURES:-} =="
# shellcheck disable=SC2086 — FEATURES is intentionally word-split
cargo build --release -p mailrs-server $FEATURES 2>&1 | tail -1
BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")/release/mailrs-server"

echo "== server: starting on :$PORT =="
mkdir -p "$WORK/maildir"
env -i PATH="$PATH" HOME="$HOME" \
  MAILRS_HOSTNAME=localhost \
  MAILRS_MAILDIR="$WORK/maildir" \
  MAILRS_WEB_PORT="$PORT" \
  MAILRS_PG_URL="$PG_URL" \
  MAILRS_LOCAL_DOMAINS=bench.local \
  MAILRS_DNSBL_ENABLED=false \
  MAILRS_ANTISPAM_ENABLED=false \
  MAILRS_AI_ANALYSIS_ENABLED=false \
  MAILRS_SMTP_PORT=0 MAILRS_SUBMISSION_PORT=0 MAILRS_IMAP_PORT=0 \
  "$BIN" > "$WORK/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 120); do
  curl -fsS "$BASE/api/health" >/dev/null 2>&1 && break
  kill -0 "$SERVER_PID" 2>/dev/null || { echo "server died — tail of log:"; tail -20 "$WORK/server.log"; exit 1; }
  sleep 0.5
done
curl -fsS "$BASE/api/health" >/dev/null || { echo "health never came up"; tail -20 "$WORK/server.log"; exit 1; }

echo "== login =="
TOKEN="$(curl -fsS -X POST "$BASE/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"address":"bench@bench.local","password":"bench-password"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['token'])")"
AUTH=(-H "Authorization: Bearer $TOKEN")

THREAD_ID="$(curl -fsS "${AUTH[@]}" "$BASE/api/conversations?limit=1" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)[0]['thread_id'])")"
echo "   token ok, probe thread: $THREAD_ID"

# timed <name> <url...>: WARMUP untimed + N timed requests, prints p50/p95/p99 ms
timed() {
  local name="$1"; shift
  for _ in $(seq 1 "$WARMUP"); do curl -fsS -o /dev/null "${AUTH[@]}" "$@" || true; done
  local times
  times="$(for _ in $(seq 1 "$N"); do
    curl -fsS -o /dev/null -w '%{time_total}\n' "${AUTH[@]}" "$@"
  done | sort -n)"
  echo "$times" | awk -v name="$name" -v n="$N" '
    { t[NR] = $1 * 1000 }
    END {
      p50 = t[int(n * 0.50) < 1 ? 1 : int(n * 0.50)]
      p95 = t[int(n * 0.95) < 1 ? 1 : int(n * 0.95)]
      p99 = t[int(n * 0.99) < 1 ? 1 : int(n * 0.99)]
      printf "%-28s p50 %8.1f ms   p95 %8.1f ms   p99 %8.1f ms\n", name, p50, p95, p99
    }'
}

echo
echo "== panel (N=$N per endpoint, sequential, warmup=$WARMUP) =="
# login is intentionally not timed: argon2-dominated (not a SQL path) and
# hammering it trips the auth-guard lockout (429)
timed "conversations?limit=50"   "$BASE/api/conversations?limit=50"
timed "conversations/{thread}"   "$BASE/api/conversations/$THREAD_ID"
timed "search?q=invoice"         "$BASE/api/conversations/search?q=invoice&limit=50"
timed "mail/stats"               "$BASE/api/mail/stats"

echo
echo "== 4-way concurrent inbox (round-22 starvation shape) =="
START=$(python3 -c "import time; print(time.time())")
for _ in 1 2 3 4; do
  curl -fsS -o /dev/null "${AUTH[@]}" "$BASE/api/conversations?limit=50" &
done
wait
python3 -c "import time; print(f'   wall: {(time.time() - $START) * 1000:.1f} ms for 4 parallel requests')"

echo
echo "done. backend=$BACKEND dataset=24304msg/23508thr work=$WORK"
