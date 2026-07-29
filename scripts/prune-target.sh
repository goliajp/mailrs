#!/usr/bin/env bash
# prune-target.sh — reclaim stale cargo build artifacts.
#
# Why this exists: on 2026-07-29 `target/` had grown to 425 GB across
# 832,531 files in `debug/deps` alone. A complete `--all-targets` build
# of this workspace is 16 GB / 31,749 files, so 96% of that was
# superseded artifacts cargo had simply never reclaimed — it does not
# garbage-collect, ever, and nothing else was doing it either.
#
# The size is only half the cost. Each full build emits ~231 fresh test
# executables, and on macOS every new executable is evaluated by
# Gatekeeper (`syspolicyd`) the first time it is exec'd. On 2026-07-29
# that queue starved an unrelated app for 204 seconds while syspolicyd
# burned 33 minutes of CPU. Fewer stale binaries is not a cure for that
# — a broken syspolicyd is macOS's problem — but there is no reason to
# keep feeding it.
#
# Policy: age-based, not size-based. `cargo sweep --maxsize` deletes by
# age until it fits, which right after a full build means deleting
# nearly everything (all timestamps are equal), so it is kept as an
# emergency ceiling rather than the routine.
#
# Usage:
#   ./scripts/prune-target.sh              # drop artifacts unused for 14 days
#   ./scripts/prune-target.sh 30           # ... 30 days instead
#   CEILING_GB=60 ./scripts/prune-target.sh
#
set -euo pipefail

DAYS="${1:-14}"
CEILING_GB="${CEILING_GB:-60}"

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

if [ ! -d target ]; then
    echo "==> no target/ directory — nothing to prune"
    exit 0
fi

if ! command -v cargo-sweep >/dev/null 2>&1; then
    echo "!! cargo-sweep not installed — run: cargo install cargo-sweep" >&2
    exit 1
fi

gb() { awk -v kb="$1" 'BEGIN { printf "%.1f", kb / 1024 / 1024 }'; }

before=$(du -sk target | cut -f1)
echo "==> target/ before: $(gb "$before") GB"

# 1. Artifacts built by a toolchain that is no longer installed can
#    never be reused — they are pure dead weight.
echo "==> dropping artifacts from uninstalled toolchains"
cargo sweep --installed "$ROOT" >/dev/null

# 2. Anything untouched for $DAYS days is a superseded hash. Cargo
#    rebuilds whatever it turns out to still need; the fingerprint
#    layer treats a missing output as "out of date", not as an error.
echo "==> dropping artifacts unused for ${DAYS} days"
cargo sweep --time "$DAYS" "$ROOT" >/dev/null

after=$(du -sk target | cut -f1)
after_gb=$((after / 1024 / 1024))
echo "==> target/ after:  $(gb "$after") GB  (reclaimed $(gb "$((before - after))") GB)"

# 3. Emergency ceiling. Reaching this means the age policy is not
#    keeping up — say so rather than silently deleting the working set.
if [ "$after_gb" -gt "$CEILING_GB" ]; then
    echo "!! still ${after_gb} GB, above the ${CEILING_GB} GB ceiling."
    echo "!! age-based pruning did not keep up. Either lower DAYS, or"
    echo "!! run: cargo sweep --maxsize ${CEILING_GB}GB $ROOT"
    echo "!! (that one deletes by age until it fits, so it will cut into"
    echo "!!  the current working set and force a full rebuild.)"
fi
