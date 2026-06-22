#!/bin/bash
# check-staging-gate.sh — staging-stricter-than-prod ship gate
#
# Reads /var/log/staging-traffic-gen.log (the always-on systemd unit
# `staging-traffic-gen.service` on t01) over the last N minutes, asserts:
#
#   - slow_rate (req ≥ 500 ms) ≤ MAX_SLOW_PCT
#   - per-endpoint max latency ≤ MAX_LATENCY_SECS
#   - min sample size — we need real traffic to gate
#
# Exit 0 = green, 1 = fail. Use in release.yml after the deploy-staging
# job completes to block prod release when staging regressed.
#
# Why these numbers (baseline captured 2026-06-22 17:58 UTC against
# v1.7.175 + spg 7.37.7 + kevy 1.26.6 on prod-shape catalog):
#
#   conv-list?limit=50   2.12% slow ≥ 500 ms / max 10.23 s
#   mail/stats           2.79% slow ≥ 500 ms / max 10.42 s
#   categories           0.62% slow / max 1.57 s
#   action-count         0.55% slow / max 4.60 s
#   overall              ≈ 1.5 % slow rate
#
# Budget = baseline × 1.3 ceiling. New ships have to STAY at-or-below
# the current cascade noise; introducing new cascade hotspots fails.
# Tighten as SPG team lands Class A/B/C planner fixes (memory
# feedback-staging-stricter-than-prod: ratchet down, never up).

set -u
LOG="${LOG:-/var/log/staging-traffic-gen.log}"
WINDOW_MIN="${WINDOW_MIN:-5}"
MAX_SLOW_PCT="${MAX_SLOW_PCT:-3.0}"          # 1.5% baseline × ~1.3 ceiling
MAX_LATENCY_SECS="${MAX_LATENCY_SECS:-15.0}" # 10s baseline + headroom
MIN_SAMPLES="${MIN_SAMPLES:-500}"            # 5 min × ~30 RPS ≈ 9000; 500 = minimum signal

if [ ! -r "$LOG" ]; then
  echo "FAIL: traffic-gen log $LOG not readable — is staging-traffic-gen.service running?"
  exit 1
fi

CUT=$(date -u -d "${WINDOW_MIN} minutes ago" '+%FT%T' 2>/dev/null)
[ -z "$CUT" ] && CUT=$(date -u '+%FT%T' -v-${WINDOW_MIN}M 2>/dev/null)
[ -z "$CUT" ] && { echo "FAIL: cannot compute time window"; exit 1; }

awk -v cut="$CUT" -v window_min="$WINDOW_MIN" \
    -v max_slow_pct="$MAX_SLOW_PCT" -v max_latency_secs="$MAX_LATENCY_SECS" \
    -v min_samples="$MIN_SAMPLES" '
  $1 >= cut {
    n_total++
    endpoint = $3
    rt = $NF + 0
    n[endpoint]++
    if (rt > max_lat[endpoint]) max_lat[endpoint] = rt
    sum_rt[endpoint] += rt
    if (rt >= 0.5) {
      slow[endpoint]++
      total_slow++
    }
    if (rt > global_max) global_max = rt
  }
  END {
    if (n_total < min_samples) {
      printf "FAIL: only %d samples in last %d min (need ≥%d) — load too low or gen broken\n",
        n_total, window_min, min_samples
      exit 1
    }
    overall_slow_pct = total_slow / n_total * 100
    printf "Window: last %d min (n=%d)\n", window_min, n_total
    printf "%-44s %8s %12s %12s %12s\n", "endpoint", "n", "mean", "max(s)", "slow≥500ms"
    bad = 0
    for (k in n) {
      mean = sum_rt[k] / n[k]
      sp = slow[k] / n[k] * 100
      tag = ""
      if (sp > max_slow_pct) { tag = tag "[SLOW%]"; bad = 1 }
      if (max_lat[k] > max_latency_secs) { tag = tag "[MAX]"; bad = 1 }
      printf "%-44s %8d %11.3fs %11.3fs %10.2f%% %s\n", k, n[k], mean, max_lat[k], sp, tag
    }
    printf "\nOverall: slow=%.2f%% (limit %.2f%%) | global_max=%.3fs (limit %.3fs)\n",
      overall_slow_pct, max_slow_pct, global_max, max_latency_secs
    if (overall_slow_pct > max_slow_pct) {
      printf "FAIL: overall slow rate %.2f%% > limit %.2f%%\n", overall_slow_pct, max_slow_pct
      bad = 1
    }
    if (global_max > max_latency_secs) {
      printf "FAIL: global max %.3fs > limit %.3fs\n", global_max, max_latency_secs
      bad = 1
    }
    if (bad) exit 1
    printf "PASS\n"
  }
' "$LOG"
