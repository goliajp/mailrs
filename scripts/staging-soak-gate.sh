#!/bin/bash
# staging-soak-gate.sh — t01 self-soak runner
#
# Fired by `staging-soak-gate.service` (systemd oneshot) at the tail of
# every deploy-staging.yml run. Sleeps SOAK_SECS (default 30 min) so the
# always-on `staging-traffic-gen.service` builds a representative window
# of post-deploy load, then runs `check-staging-gate.sh` against the
# rolling traffic-gen log and writes a verdict to STATUS_FILE.
#
# release.yml's prod-deploy job reads STATUS_FILE over SSH and refuses
# to ship when sha != current tag's commit, or pass != true, or the
# verdict is older than MAX_AGE_SECS. See
# `~/.claude-profile-1/projects/*/memory/feedback-staging-stricter-than-prod.md`.

set -u
SOAK_SECS="${SOAK_SECS:-1800}"
WINDOW_MIN="${WINDOW_MIN:-15}"
STATUS_FILE="${STATUS_FILE:-/var/run/staging-gate.json}"
DEPLOY_SHA_FILE="${DEPLOY_SHA_FILE:-/etc/staging-deploy-sha}"
LOG_FILE="${LOG_FILE:-/var/log/staging-traffic-gen.log}"
MAX_SLOW_PCT="${MAX_SLOW_PCT:-3.0}"
MAX_LATENCY_SECS="${MAX_LATENCY_SECS:-15.0}"
MIN_SAMPLES="${MIN_SAMPLES:-5000}"  # 15 min × ~30 RPS ≈ 27000; floor at 5k for sanity

SHA="$(cat "$DEPLOY_SHA_FILE" 2>/dev/null || echo unknown)"
DEPLOY_TS=$(date -u +%s)

logger -t staging-soak-gate "started sha=$SHA soak_secs=$SOAK_SECS window_min=$WINDOW_MIN"
sleep "$SOAK_SECS"

GATE_TS=$(date -u +%s)
GATE_OUT="$(LOG="$LOG_FILE" WINDOW_MIN="$WINDOW_MIN" \
  MAX_SLOW_PCT="$MAX_SLOW_PCT" MAX_LATENCY_SECS="$MAX_LATENCY_SECS" \
  MIN_SAMPLES="$MIN_SAMPLES" \
  /usr/local/bin/check-staging-gate.sh 2>&1)"
GATE_RC=$?

# parse aggregate stats out of check-staging-gate.sh's "Overall:" line:
#   Overall: slow=1.49% (limit 3.00%) | global_max=10.416s (limit 15.000s)
SLOW_PCT=$(echo "$GATE_OUT" | sed -nE 's/^Overall:.*slow=([0-9.]+)%.*/\1/p' | head -1)
MAX_LAT=$(echo "$GATE_OUT" | sed -nE 's/^Overall:.*global_max=([0-9.]+)s.*/\1/p' | head -1)
SAMPLE_N=$(echo "$GATE_OUT" | sed -nE 's/^Window:.*n=([0-9]+).*/\1/p' | head -1)

PASS=false
[ "$GATE_RC" = "0" ] && PASS=true

cat > "$STATUS_FILE.tmp" <<EOF
{
  "sha": "$SHA",
  "deploy_ts": $DEPLOY_TS,
  "gate_ts": $GATE_TS,
  "pass": $PASS,
  "slow_pct": "${SLOW_PCT:-null}",
  "max_lat_secs": "${MAX_LAT:-null}",
  "sample_n": ${SAMPLE_N:-0},
  "limit_slow_pct": $MAX_SLOW_PCT,
  "limit_max_lat_secs": $MAX_LATENCY_SECS,
  "window_min": $WINDOW_MIN
}
EOF
mv "$STATUS_FILE.tmp" "$STATUS_FILE"

logger -t staging-soak-gate "verdict pass=$PASS sha=$SHA slow_pct=${SLOW_PCT:-?} max=${MAX_LAT:-?}"

# also stash the full gate output for forensic
mkdir -p /var/log/staging-soak-gate
echo "$GATE_OUT" > "/var/log/staging-soak-gate/$(date -u +%Y%m%dT%H%M%SZ)-$SHA.txt"

[ "$PASS" = "true" ] && exit 0 || exit 1
