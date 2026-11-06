#!/usr/bin/env bash
# perf-gates.sh — run the perf regression gates where their numbers mean
# something, i.e. in release, on a machine that is not busy.
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
# A latency gate on a busy machine measures the machine. On studio with
# every core taken, the parse worker's P95 read four times its quiet
# figure with no code change (2.213.0 deploy). So each gate binary runs
# alone, load1 is sampled before, during and after it, and a failure
# during which load1 went above PERF_GATE_MAX_LOAD is void: it is run
# again once load1 is back under the threshold. A pass under load
# stands, since load only makes latency worse. When no quiet run is
# possible within PERF_GATE_MAX_WAIT seconds of waiting in total, the
# script exits 75 and says so; that is neither a pass nor a failure.
#
# Run this before a release, or after touching anything on a hot path.
#
# Usage:
#   ./scripts/perf-gates.sh              # every crate that has a gate
#   ./scripts/perf-gates.sh mailrs-dkim  # just one
#
# Environment:
#   PERF_GATE_MAX_LOAD   load1 above which a failure is void (default 16,
#                        studio's core count; its ordinary load1 is ~12)
#   PERF_GATE_MAX_WAIT   seconds of waiting for a quiet machine, in
#                        total, before giving up (default 1200)
#   PERF_GATE_POLL_SECS  seconds between load checks while waiting (15)
#   PERF_GATE_SAMPLE_SECS seconds between samples during a run (2)
#   PERF_GATE_LOAD_FILE  read load1 from this file instead of the
#                        system (for testing the script)
#   PERF_GATE_BINARIES   skip the build and run the gates listed in this
#                        file, one "<dir><TAB><executable>" per line (for
#                        testing the script)
#
set -euo pipefail

cd "$(dirname "$0")/.."

MAX_LOAD=${PERF_GATE_MAX_LOAD:-16}
MAX_WAIT=${PERF_GATE_MAX_WAIT:-1200}
POLL=${PERF_GATE_POLL_SECS:-15}
SAMPLE=${PERF_GATE_SAMPLE_SECS:-2}
BUSY_EXIT=75

load1() {
    if [ -n "${PERF_GATE_LOAD_FILE:-}" ]; then
        cat "$PERF_GATE_LOAD_FILE"
    elif [ -r /proc/loadavg ]; then
        cut -d' ' -f1 /proc/loadavg
    else
        # macOS: "{ 12.34 11.00 10.50 }"
        sysctl -n vm.loadavg | awk '{print $2}'
    fi
}

above() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a > b) }'; }

waited=0
busiest=0

# Block until load1 is at or under the threshold, counting against the
# total wait; give up with BUSY_EXIT when the total runs out.
wait_for_quiet() {
    local now
    while now=$(load1) && above "$now" "$MAX_LOAD"; do
        if above "$now" "$busiest"; then busiest=$now; fi
        if [ "$waited" -ge "$MAX_WAIT" ]; then
            echo "!! machine too busy to judge perf (load1 max $busiest > threshold $MAX_LOAD)"
            exit "$BUSY_EXIT"
        fi
        echo "    load1 $now > $MAX_LOAD, waiting (${waited}s of ${MAX_WAIT}s)"
        sleep "$POLL"
        waited=$((waited + POLL))
    done
}

# Run one gate binary with load1 sampled around it. Sets `rc` to the
# binary's exit status and `peak` to the highest load1 seen.
run_sampled() {
    local dir=$1 exe=$2 samples sampler
    samples=$(mktemp -t perf-gate-load.XXXXXX)
    load1 > "$samples"
    ( while :; do sleep "$SAMPLE"; load1 >> "$samples"; done ) &
    sampler=$!
    rc=0
    (cd "$dir" && "$exe") || rc=$?
    kill "$sampler"
    wait "$sampler" 2>/dev/null || true
    load1 >> "$samples"
    peak=$(sort -g "$samples" | tail -1)
    rm -f "$samples"
}

# "<dir>\t<executable>" for every perf_gate test binary, built in release
gate_binaries() {
    if [ -n "${PERF_GATE_BINARIES:-}" ]; then
        cat "$PERF_GATE_BINARIES"
        return
    fi
    cargo test --release "$@" --test perf_gate --no-run --message-format=json \
        | jq -r 'select(.reason == "compiler-artifact" and .target.name == "perf_gate"
                        and .executable != null)
                 | "\(.manifest_path | sub("/Cargo.toml$"; ""))\t\(.executable)"'
}

if [ $# -gt 0 ]; then
    # Package selectors, not name filters. `cargo test --test perf_gate
    # mailrs-mime` reads the crate name as a filter, runs nothing, and
    # still exits 0 — "0 passed; 4 filtered out" looks like a pass.
    sel=()
    for p in "$@"; do sel+=(-p "$p"); done
    echo "==> perf gates (release): $*"
else
    sel=(--workspace)
    echo "==> perf gates (release), whole workspace"
    echo "    first run builds in release and takes a while"
fi
echo "    a failure is void when load1 goes above $MAX_LOAD (PERF_GATE_MAX_LOAD)"

list=$(mktemp -t perf-gate-bins.XXXXXX)
gate_binaries "${sel[@]}" > "$list"
if [ ! -s "$list" ]; then
    echo "!! no perf_gate binaries were built"
    exit 1
fi

failed=()
while IFS=$'\t' read -r dir exe <&3; do
    name=${dir##*/}
    while :; do
        wait_for_quiet
        echo "==> $name"
        started=$SECONDS
        run_sampled "$dir" "$exe"
        # a pass under load stands: load only makes latency worse
        if [ "$rc" -ne 0 ] && above "$peak" "$MAX_LOAD"; then
            if above "$peak" "$busiest"; then busiest=$peak; fi
            # a void run is time spent waiting too, or a machine that is
            # quiet between runs and busy during them would loop forever
            waited=$((waited + SECONDS - started))
            if [ "$waited" -ge "$MAX_WAIT" ]; then
                echo "!! machine too busy to judge perf (load1 max $busiest > threshold $MAX_LOAD)"
                exit "$BUSY_EXIT"
            fi
            echo "    void: failed while load1 reached $peak (> $MAX_LOAD); running again when quiet"
            continue
        fi
        break
    done
    if [ "$rc" -ne 0 ]; then failed+=("$name"); fi
done 3< "$list"
rm -f "$list"

if [ ${#failed[@]} -gt 0 ]; then
    echo "!! perf gates failed on a quiet machine: ${failed[*]}"
    exit 1
fi
echo "==> perf gates passed"
