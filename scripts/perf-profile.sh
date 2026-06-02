#!/usr/bin/env bash
# usage: ./scripts/perf-profile.sh <crate-short-name> [bench-name]
#
# v4 perf-squeeze harness. Profiles one stone's criterion bench using
# samply (default) and produces a flamegraph + per-fn breakdown that
# the v4 hot plan consumes.
#
# Expects:
#   - samply installed (`cargo install samply`)
#   - the crate has at least one criterion bench in benches/
#   - $CARGO_TARGET_DIR points at the workspace target dir
#
# Outputs to /tmp/perf-<crate>-<bench>/ :
#   profile.json   — samply profile (load in profiler.firefox.com)
#   summary.txt    — top-50 self-time functions
#   bench.out      — raw criterion output (median / p95 / p99)
#
# After running, hand the summary.txt to the optimization loop. The
# loop's job is to identify hot inner functions, compare them against
# the named competitor in PERFORMANCE.md, and lower them.

set -euo pipefail

CRATE="${1:-}"
BENCH="${2:-}"
if [ -z "$CRATE" ]; then
  echo "usage: $0 <crate-short-name> [bench-name]"
  echo "       e.g.   $0 smtp-proto parse"
  echo "       e.g.   $0 rfc5322"
  exit 1
fi

CARGO_TARGET="${CARGO_TARGET_DIR:-./target}"
CRATE_DIR="crates/$CRATE"
if [ ! -d "$CRATE_DIR" ]; then
  echo "error: $CRATE_DIR does not exist"
  exit 1
fi

PKG="mailrs-$CRATE"
OUT_DIR="/tmp/perf-$CRATE${BENCH:+-$BENCH}"
mkdir -p "$OUT_DIR"

echo "==> building release bench for $PKG"
if [ -n "$BENCH" ]; then
  cargo build --release -p "$PKG" --bench "$BENCH"
else
  cargo build --release -p "$PKG" --benches
fi

# Locate the freshly-built bench binary. Criterion compiles each
# bench file `benches/foo.rs` into target/release/deps/foo-<hash>.
# When BENCH is unspecified, default to the crate short-name in
# snake_case (e.g. crate `smtp-codec` → bench bin `smtp_codec-<hash>`).
# `mailrs_<name>-<hash>` is the unittest runner, NOT the bench bin.
BENCH_GLOB="${BENCH:-${CRATE//-/_}}"
BENCH_BIN=$(find "$CARGO_TARGET/release/deps" -maxdepth 1 -type f -perm -u+x \
  -name "${BENCH_GLOB//-/_}-*" \
  ! -name "*.d" ! -name "*.rmeta" ! -name "*.o" ! -name "*.rlib" ! -name "*.dylib" \
  ! -name "lib*" ! -name "mailrs_*" \
  -mmin -60 \
  | head -1)
if [ -z "$BENCH_BIN" ]; then
  echo "error: no recently-built bench binary found for $BENCH_GLOB"
  exit 1
fi
echo "==> bench: $BENCH_BIN"

# 1. raw criterion run for baseline numbers
echo "==> criterion baseline"
"$BENCH_BIN" --bench 2>&1 | tee "$OUT_DIR/bench.out"

# 2. samply profile — capture call-tree
echo "==> samply record"
if ! command -v samply >/dev/null 2>&1; then
  echo "error: samply not installed. cargo install samply"
  exit 1
fi
samply record --save-only -o "$OUT_DIR/profile.json" -- "$BENCH_BIN" --bench

# samply 0.13+ no longer emits a text summary from `samply load` —
# it always starts a web server. Skip the auto-summary step; the
# perf-squeeze loop opens the profile in profiler.firefox.com directly.

echo ""
echo "==> done. artifacts in $OUT_DIR"
ls -la "$OUT_DIR"
echo ""
echo "==> open the profile in profiler.firefox.com (will start a web server):"
echo "    samply load $OUT_DIR/profile.json"
