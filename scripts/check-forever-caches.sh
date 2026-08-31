#!/usr/bin/env bash
# A cache with no expiry is a claim that nothing can change the thing
# it holds. Every such claim has to name the writers it checked.
#
# On 2026-09-01 two of the three in this tree were false. The thread
# query said "the only thing that can change a thread's content is an
# inbound message, and that arrives over the WebSocket" — true for a
# connected tab, and the cache is persisted, so a tab that was closed
# when the reply landed showed two messages of three through every
# reload. The fraud verdict said "never changes after it is written",
# and `maintenance:fraud-rescan` rewrites it on every run.
#
# Not being able to see mail is the same class of failure as not
# receiving it, so this is a gate rather than a review note.
#
# The rule: a `staleTime: Infinity` must carry `writers-checked:` in
# the comment above it, naming what was enumerated and when.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
while IFS=: read -r file line _; do
    [ -z "$file" ] && continue
    # The eight lines above the match, where the justification lives.
    start=$(( line > 8 ? line - 8 : 1 ))
    if ! sed -n "${start},$((line - 1))p" "$file" | grep -qi "writers-checked:"; then
        echo "!! $file:$line — staleTime: Infinity with no 'writers-checked:' above it"
        fail=1
    fi
done < <(grep -rn "staleTime: Infinity" --include='*.ts' --include='*.tsx' web/src/ | grep -v '//.*staleTime: Infinity')

if [ "$fail" -eq 0 ]; then
    n=$(grep -rn "staleTime: Infinity" --include='*.ts' --include='*.tsx' web/src/ | grep -vc '//.*staleTime: Infinity')
    echo "forever-caches OK — $n justified"
fi
exit "$fail"
