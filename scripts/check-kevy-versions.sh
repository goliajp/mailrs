#!/usr/bin/env bash
# Every kevy crate in the lockfile is one version, and that version is
# the one the manifests ask for.
#
# Two generations of a kevy crate in one graph is not a warning: the
# types are structurally identical and differ only by which copy they
# came from, so it surfaces as `expected ValType, found IndexValType`
# a hundred lines from the cause — if it surfaces at all. `kevy-index`
# is depended on directly (kevy-embedded does not re-export
# `TableSpec`), and its manifest comment has said "pinned to
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

[ "$fail" -eq 0 ] && echo "kevy versions OK — all on $(echo "$gens" | tr -d '\n')"
exit "$fail"
