#!/usr/bin/env bash
# Compare SMTP receive throughput between the perf-first release profile
# (lto=fat, cgu=1, panic=abort) and a vanilla release profile (defaults
# restored). See PERFORMANCE.md row for commit 9f21e0b and
# crates/server/benches/smtp_load.rs for methodology.
#
# Usage:
#   scripts/bench-smtp-load.sh [duration_s] [conns] [rounds]
# Defaults: duration_s=30 conns=32 rounds=3
#
# Output: one line per round per profile, then a summary table with
# median / min / max across rounds and the perf-first vs vanilla delta.

set -euo pipefail

DURATION="${1:-30}"
CONNS="${2:-32}"
ROUNDS="${3:-3}"
WARMUP="${WARMUP:-3}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Locate bench binaries via cargo's JSON output — target-dir may be
# local, the wrapper's fallback, or external (per cargo-target-dir.md).
locate_bench() {
  local profile="$1"
  cargo build --profile "$profile" -p mailrs-server --bench smtp_load \
    --message-format=json 2>/dev/null \
    | python3 -c "
import json, sys
for line in sys.stdin:
  try:
    d = json.loads(line)
    if d.get('reason') == 'compiler-artifact' and 'smtp_load' in d.get('target', {}).get('name', ''):
      print(d.get('executable'))
  except Exception:
    pass" | tail -1
}

echo "==> building perf-first profile (cargo build --release)"
PERF_BIN=$(locate_bench release)
echo "    binary: $PERF_BIN"

echo "==> building vanilla profile (cargo build --profile release-vanilla)"
VANILLA_BIN=$(locate_bench release-vanilla)
echo "    binary: $VANILLA_BIN"

if [[ ! -x "$PERF_BIN" || ! -x "$VANILLA_BIN" ]]; then
  echo "ERROR: failed to locate one or both bench binaries" >&2
  exit 1
fi

PERF_SIZE=$(du -h "$PERF_BIN" | awk '{print $1}')
VANILLA_SIZE=$(du -h "$VANILLA_BIN" | awk '{print $1}')
echo "    perf-first binary size:    $PERF_SIZE"
echo "    vanilla binary size:       $VANILLA_SIZE"
echo ""

# Use --no-deliver to isolate CPU-side work. The Maildir write involves
# fsync per message which makes the bench disk-bound and masks the CPU
# delta we're trying to measure (LTO / CGU / panic). See bench file
# docstring "no-deliver mode" section for full rationale. Override with
# EXTRA="" to bench full delivery path.
EXTRA="${EXTRA---no-deliver}"

run_round() {
  local bin="$1" label="$2" round="$3"
  echo "-- $label round $round --" >&2
  # shellcheck disable=SC2086
  "$bin" --duration "$DURATION" --conns "$CONNS" --warmup "$WARMUP" --label "$label" $EXTRA
}

PERF_LINES=()
VAN_LINES=()

# Interleave perf-first / vanilla rounds so any thermal / noisy-neighbor
# drift spreads evenly across both.
for round in $(seq 1 "$ROUNDS"); do
  PERF_LINES+=("$(run_round "$PERF_BIN" perf-first "$round")")
  VAN_LINES+=("$(run_round "$VANILLA_BIN" vanilla "$round")")
done

echo ""
echo "==> raw rounds"
for line in "${PERF_LINES[@]}"; do echo "$line"; done
for line in "${VAN_LINES[@]}";  do echo "$line"; done

echo ""
echo "==> summary (median / min / max across $ROUNDS rounds)"
PERF=$(printf '%s\n' "${PERF_LINES[@]}") VAN=$(printf '%s\n' "${VAN_LINES[@]}") \
python3 - <<'PY'
import os, re

def parse(lines):
    out = []
    for L in lines:
        m = re.search(r'throughput_msg_s=([\d.]+).*p50_us=(\d+).*p99_us=(\d+).*p999_us=(\d+)', L)
        if m:
            out.append((float(m.group(1)), int(m.group(2)), int(m.group(3)), int(m.group(4))))
    return out

def med(xs):
    s = sorted(xs)
    n = len(s)
    if n % 2:
        return s[n // 2]
    return (s[n // 2 - 1] + s[n // 2]) / 2

perf = parse(os.environ.get('PERF','').splitlines())
vanilla = parse(os.environ.get('VAN','').splitlines())

def fmt(rows, name):
    if not rows:
        print(f"{name}: no successful rounds")
        return None
    tps = [r[0] for r in rows]
    p50 = [r[1] for r in rows]
    p99 = [r[2] for r in rows]
    p999 = [r[3] for r in rows]
    print(f"{name:12s} throughput_msg_s med={med(tps):.1f} min={min(tps):.1f} max={max(tps):.1f}")
    print(f"             p50_us         med={int(med(p50))} min={min(p50)} max={max(p50)}")
    print(f"             p99_us         med={int(med(p99))} min={min(p99)} max={max(p99)}")
    print(f"             p999_us        med={int(med(p999))} min={min(p999)} max={max(p999)}")
    return med(tps), med(p99)

pf = fmt(perf, "perf-first")
vn = fmt(vanilla, "vanilla")

if pf and vn:
    tp_pf, p99_pf = pf
    tp_vn, p99_vn = vn
    print()
    print(f"throughput delta (perf-first vs vanilla): {(tp_pf - tp_vn) / tp_vn * 100:+.2f}%")
    print(f"p99 latency delta (perf-first vs vanilla): {(p99_pf - p99_vn) / p99_vn * 100:+.2f}%")
PY
