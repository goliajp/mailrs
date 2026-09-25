#!/usr/bin/env bash
# Every kevy crate in the lockfile is one version, and that version is
# the one the manifests ask for.
#
# Two generations of a kevy crate in one graph is not a warning: the
# types are structurally identical and differ only by which copy they
# came from, so it surfaces as `expected ValType, found IndexValType`
# a hundred lines from the cause — if it surfaces at all. `kevy-index`
# is depended on directly (kevy-embedded re-exports `TableSpec` but not
# the query-clause types `WhereClause` / `CompositeCol` /
# `composite_bounds`), and its manifest comment has said "pinned to
# kevy-embedded's version, deliberately" since it was written. On
# 2026-08-31 the workspace went to 6.2.1 and that pin stayed at 5.4.
# A comment stating an invariant does not maintain it.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
dupes=$(awk '/^name = "kevy/ { n=$3 } /^version = / && n { print n, $3; n="" }' Cargo.lock \
        | sort -u | awk '{ c[$1]++; v[$1]=v[$1]" "$2 } END { for (k in c) if (c[k]>1) print k, v[k] }')
if [ -n "$dupes" ]; then
    echo "!! more than one version of a kevy crate in Cargo.lock:"
    echo "$dupes" | sed 's/^/   /'
    fail=1
fi

# And they must all be the same generation as each other.
gens=$(awk '/^name = "kevy/ { n=$3 } /^version = / && n { print $3; n="" }' Cargo.lock \
       | tr -d '"' | cut -d. -f1 | sort -u)
if [ "$(echo "$gens" | wc -l | tr -d ' ')" -gt 1 ]; then
    echo "!! kevy crates span more than one major:" "$(echo "$gens" | tr '\n' ' ')"
    fail=1
fi

# The server container is not implied by the crate: `kevy-client` talks to
# whatever `deploy/docker-compose.prod.yml` pins, and the integration test
# that exists to cover that pair keeps its own default tag. Both have gone
# stale before — the compose moved to 6.x while
# `crates/server/tests/kevy_network.rs` stayed at 3.18.0, so the suite
# tested a client/server pair production has never run, under a comment
# saying why that must not happen.
crate_ver=$(awk -F'"' '/^kevy-embedded = "/ { print $2; exit }' Cargo.toml)
compose_ver=$(awk -F: '/image: ghcr.io\/goliajp\/kevy:/ { print $NF; exit }' deploy/docker-compose.prod.yml)
test_ver=$(awk -F'"' '/env::var\("MAILRS_TEST_KEVY_TAG"\)/ { print $4; exit }' crates/server/tests/kevy_network.rs)

if [ "$compose_ver" != "$crate_ver" ]; then
    echo "!! kevy-server in deploy/docker-compose.prod.yml is $compose_ver, crates ask for $crate_ver"
    fail=1
fi
if [ "$test_ver" != "$compose_ver" ]; then
    echo "!! crates/server/tests/kevy_network.rs defaults to $test_ver, prod compose runs $compose_ver"
    fail=1
fi

[ "$fail" -eq 0 ] && echo "kevy versions OK — crates, prod compose and the network test all on $crate_ver"
exit "$fail"
