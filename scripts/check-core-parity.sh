#!/usr/bin/env bash
# check-core-parity.sh — hold the two cores to one core-api contract.
#
# mailrs serves the core-api contract from two interchangeable cores, and
# `MAILRS_CORE_RPC_BASE` decides which one answers:
#
#   fastcore  (kevy, in-process)   crates/fastcore/src/router/
#   pg-core   (pg/spg SQL)         crates/server/src/core_rpc/
#
# A path one core mounts and the other does not is a feature that works or
# 404s depending on which is deployed. This asserts the relationship in
# both directions, with different strictness for each:
#
#   pg-core ⊆ fastcore   HARD FAIL. fastcore is what production runs; a
#                        route the dormant lane serves and prod does not
#                        is a straight regression.
#   fastcore \ pg-core   RATCHET against scripts/core-parity-baseline.txt.
#                        This set is the pg-core reactivation debt. It may
#                        shrink, never grow. Run with --update after
#                        closing one.
#
# Why a ratchet and not equality: the two sets are genuinely unequal today,
# and a gate that is red on arrival is a gate nobody reads — the same reason
# `kevy-cli doctor` warns rather than fails on `duplicates`.
#
# This replaced core_rpc/tests/parity.rs, which had rotted three ways at
# once and could not report any of them: it `include_str!`'d
# `../../../fastcore/src/lib.rs` (a path that does not exist — the router
# moved to router/mod.rs, and the old path resolved to crates/server/
# fastcore/), it searched for `"fn build_full_router"` in mod.rs after that
# string moved to router.rs, and it bounded the function body with the first
# top-level `}` rather than by brace depth. None of it fired, because it sat
# behind `--features core-rpc`, which nothing built after 2026-08-02.
# Two consequences shaped this script:
#
#   - it discovers router files BY CONTENT, never by path, so the next file
#     split cannot blind it (the lesson `check-mcp-parity.sh` carries too);
#   - it is plain grep, so it runs in the everyday gate instead of only
#     under a cargo feature. A check that needs a feature nobody builds is
#     how the lane rotted in the first place.
#
# Exit 0 = pg-core ⊆ fastcore and the debt has not grown.
set -euo pipefail
cd "$(dirname "$0")/.."

# Byte order everywhere. `comm` only answers correctly when both inputs are
# sorted the same way, and python's `sorted()` is byte order while `sort`
# under en_US.UTF-8 collates punctuation loosely — the two disagree on
# `PATH_MARK_NOTIFICATION` vs `PATH_MARK_NOT_JUNK` (`I` < `_` by byte, the
# other way by locale). Without this the gate reported one baselined route
# as new debt on a clean tree, which reads exactly like a real finding.
export LC_ALL=C

BASELINE=scripts/core-parity-baseline.txt

# Found, not named: a `*.rs` glob stops at the directory it names, so the
# first route moved into a subdirectory would leave this comparing against
# a lane it can no longer fully see — and reporting parity.
lane() { grep -rl --include='*.rs' '\.route(' "$1" 2>/dev/null | sort; }

extract() {
    # Print the PATH_* constant each `.route(` call registers.
    #
    # Only the constant, not the module alias in front of it: the two lanes
    # alias the same core-api modules differently (`th_paths::` vs
    # `thread::`), and the constant is the contract.
    #
    # Routes spelled as string literals are deliberately skipped. They are
    # fastcore's `/v1/admin/maintenance:*` family, which is not part of the
    # core-api contract and has no pg-core counterpart by design.
    python3 - "$@" <<'PY'
import re, sys
names = []
for path in sys.argv[1:]:
    with open(path) as fh:
        src = fh.read()
    for m in re.finditer(r'\.route\(', src):
        seg = src[m.end():m.end() + 240]
        p = re.search(r'\bPATH_[A-Z0-9_]+', seg)
        if p:
            names.append(p.group(0))
print('\n'.join(sorted(set(names))))
PY
}

PG=$(extract $(lane crates/server/src/core_rpc))
FC=$(extract $(lane crates/fastcore/src))

n_pg=$(printf '%s\n' "$PG" | grep -c . || true)
n_fc=$(printf '%s\n' "$FC" | grep -c . || true)

pg_only=$(comm -23 <(printf '%s\n' "$PG") <(printf '%s\n' "$FC") | grep . || true)
fc_only=$(comm -13 <(printf '%s\n' "$PG") <(printf '%s\n' "$FC") | grep . || true)

if [ "${1:-}" = "--update" ]; then
    printf '%s\n' "$fc_only" > "$BASELINE"
    echo "baseline updated: $(printf '%s\n' "$fc_only" | grep -c . || true) routes fastcore serves and pg-core does not"
    exit 0
fi

echo "pg-core:  $n_pg contract routes"
echo "fastcore: $n_fc contract routes"

fail=0

if [ -n "$pg_only" ]; then
    echo
    echo "!! REGRESSION — pg-core serves routes fastcore does not:"
    printf '    %s\n' $pg_only
    echo
    echo "fastcore is what production answers with. Add these there rather"
    echo "than removing them from pg-core."
    fail=1
fi

if [ ! -f "$BASELINE" ]; then
    echo
    echo "!! no baseline at $BASELINE — run: $0 --update"
    exit 1
fi

new=$(comm -13 <(sort "$BASELINE" | grep . || true) <(printf '%s\n' "$fc_only") | grep . || true)
closed=$(comm -23 <(sort "$BASELINE" | grep . || true) <(printf '%s\n' "$fc_only") | grep . || true)
n_debt=$(printf '%s\n' "$fc_only" | grep -c . || true)
n_base=$(grep -c . "$BASELINE" || true)

if [ -n "$new" ]; then
    echo
    echo "!! DEBT GREW — new routes on fastcore with no pg-core counterpart:"
    printf '    %s\n' $new
    echo
    echo "A capability on one side of the wire and nowhere on the other looks"
    echo "finished from both sides. Mount it on pg-core in the same change, or"
    echo "run '$0 --update' if the omission is deliberate and recorded."
    fail=1
fi

[ "$fail" = 1 ] && exit 1

if [ -n "$closed" ]; then
    echo
    echo "pg-core reactivation debt shrank by $(printf '%s\n' "$closed" | grep -c .):"
    printf '    %s\n' $closed
    echo
    echo "Run '$0 --update' to retire these from the baseline."
    exit 0
fi

echo "core parity OK — pg-core ⊆ fastcore; reactivation debt $n_debt/$n_base routes"
