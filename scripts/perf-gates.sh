#!/usr/bin/env bash
# perf-gates.sh — run the perf regression gates where their numbers mean
# something, i.e. in release.
#
# Every `crates/*/tests/perf_gate.rs` budget was derived from an
# optimised build. Asserting them in a dev build measures how contended
# the host is: on 2026-07-29 dkim (6.75 µs against a 5 µs budget) and
# mime each went red inside `cargo test --workspace` — which runs
# hundreds of test binaries at once — while passing ten out of ten when
# run alone, and 40/40 under a load average of 15.
#
# Half the files had already been worked around by inflating budgets
# until they survived debug. That trades away the regression the gate
# exists to catch. The other half now compile only in release
# (`#![cfg(not(debug_assertions))]`) with their numbers untouched, which
# is why this script exists — a gate nobody runs is worth less than a
# flaky one, because at least a flaky one is visible.
#
# Run this before a release, or after touching anything on a hot path.
#
# Usage:
#   ./scripts/perf-gates.sh              # every crate that has a gate
#   ./scripts/perf-gates.sh mailrs-dkim  # just one
#
set -euo pipefail

cd "$(dirname "$0")/.."

if [ $# -gt 0 ]; then
    # Package selectors, not name filters. `cargo test --test perf_gate
    # mailrs-mime` reads the crate name as a filter, runs nothing, and
    # still exits 0 — "0 passed; 4 filtered out" looks like a pass.
    pkgs=()
    for p in "$@"; do pkgs+=(-p "$p"); done
    echo "==> perf gates (release): $*"
    exec cargo test --release "${pkgs[@]}" --test perf_gate
fi

echo "==> perf gates (release), whole workspace"
echo "    first run builds in release and takes a while"
cargo test --release --workspace --test perf_gate --no-fail-fast
