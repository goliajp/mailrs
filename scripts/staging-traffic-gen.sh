#!/bin/bash
# staging traffic generator — t01 internal
# replays prod cascade-prone endpoints against mailrs-staging on
# prod-shape catalog. systemd unit / cron / `nohup &` it.
#
# Knobs (env):
#   STAGING_URL    base url (default http://localhost:3103 — webapi-fc/fastcore)
#   ADDR           login address (default lihao@golia.jp — works on
#                  prod-shape catalog)
#   PASSWORD       login password
#   CONCURRENCY    parallel workers (default 8 — beyond prod typical 2-4)
#   RPS_PER_WORKER per-worker sustained rate (default 5)
#   LOG_FILE       where to write per-request latency (default /var/log/staging-traffic-gen.log)
#   STATS_FILE     periodic budget summary (default /var/log/staging-traffic-gen-stats.log)

set -u
STAGING_URL="${STAGING_URL:-http://localhost:3103}"
ADDR="${ADDR:-lihao@golia.jp}"
PASSWORD="${PASSWORD:?PASSWORD required (set in /etc/staging-traffic-gen.env)}"
CONCURRENCY="${CONCURRENCY:-8}"
RPS_PER_WORKER="${RPS_PER_WORKER:-5}"
LOG_FILE="${LOG_FILE:-/var/log/staging-traffic-gen.log}"
STATS_FILE="${STATS_FILE:-/var/log/staging-traffic-gen-stats.log}"

mkdir -p "$(dirname "$LOG_FILE")"
: > "$LOG_FILE"
: > "$STATS_FILE"

log() {
  echo "$(date -u +%FT%TZ) gen: $*" | tee -a "$STATS_FILE" >&2
}

login() {
  curl -sS -X POST "$STAGING_URL/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"address\":\"$ADDR\",\"password\":\"$PASSWORD\"}" \
    -o /tmp/staging-login-$$.json -w "%{http_code}" 2>/dev/null
}

extract_token() {
  grep -o '"token":"[^"]*"' /tmp/staging-login-$$.json | head -1 | sed 's/"token":"//;s/"//'
}

# log + refresh token every ~10 min (web session TTL is hours; 10m is comfortable)
refresh_token() {
  rc=$(login)
  if [ "$rc" != "200" ]; then
    log "login failed http=$rc"
    return 1
  fi
  TOKEN=$(extract_token)
  if [ -z "$TOKEN" ] || [ ${#TOKEN} -lt 10 ]; then
    log "token extract failed"
    return 1
  fi
  log "token refreshed len=${#TOKEN}"
  echo "$TOKEN" > /tmp/staging-token
  return 0
}

# worker function: loop, each iter hits one of N cascade-prone endpoints
worker() {
  worker_id="$1"
  delay_ms=$((1000 / RPS_PER_WORKER))
  while :; do
    TOKEN=$(cat /tmp/staging-token 2>/dev/null)
    [ -z "$TOKEN" ] && sleep 2 && continue

    # rotate through the 4 endpoints prod cascade hit
    case $((RANDOM % 4)) in
      0) ENDPOINT="/api/conversations?limit=50" ;;
      1) ENDPOINT="/api/conversations/categories" ;;
      2) ENDPOINT="/api/conversations/action-count" ;;
      3) ENDPOINT="/api/mail/stats" ;;
    esac

    out=$(curl -sS -m 30 -o /dev/null -w "%{http_code} %{time_total}" \
      "$STAGING_URL$ENDPOINT" -H "Authorization: Bearer $TOKEN" 2>/dev/null)
    echo "$(date -u +%FT%T.%3NZ) w$worker_id $ENDPOINT $out" >> "$LOG_FILE"

    sleep 0.$((delay_ms < 10 ? 0 : delay_ms / 10))
  done
}

# stats reporter: every 60s, summarise last minute
stats_reporter() {
  while :; do
    sleep 60
    if [ -s "$LOG_FILE" ]; then
      MIN_AGO=$(date -u -d '60 seconds ago' '+%FT%T' 2>/dev/null)
      [ -z "$MIN_AGO" ] && MIN_AGO=$(date -u '+%FT%T' -v-60S 2>/dev/null)
      total=$(tail -n 10000 "$LOG_FILE" | awk -v cut="$MIN_AGO" '$1 >= cut' | wc -l)
      slow=$(tail -n 10000 "$LOG_FILE" | awk -v cut="$MIN_AGO" '$1 >= cut {split($NF, a, " "); v=$NF+0; if (v >= 0.5) print}' | wc -l)
      pct_slow="0"
      [ "$total" -gt 0 ] && pct_slow=$(awk -v s=$slow -v t=$total 'BEGIN{printf "%.1f", s/t*100}')
      log "1m: total=$total slow_ge500ms=$slow ($pct_slow%)"
    fi
  done
}

# retry with backoff — exiting on first failure turns systemd restart
# into a login storm that trips the auth rate limiter (429) forever
until refresh_token; do
  log "initial login failed; retrying in 60s"
  sleep 60
done
# bg the workers
i=0
while [ "$i" -lt "$CONCURRENCY" ]; do
  worker "$i" &
  i=$((i + 1))
done
stats_reporter &

# refresh token loop
while :; do
  sleep 600
  refresh_token || log "token refresh failed, will retry in 60s" && sleep 60
done
