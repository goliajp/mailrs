#!/usr/bin/env bash
# Bundle, review a read-only corpus diff, then activate without rebuilding.
# Usage: deploy-fraud-rules.sh BUNDLE [--activate]
# Build a bundle the way DEFAULT_SOURCE is built, each file followed by a newline:
#   for f in brands helpers identity provenance content; do
#     cat "crates/fraud-lua/rules/$f.lua"; printf '\n'; done > /tmp/fraud.lua
# --check takes any bundle. --activate takes only the bundle committed on
# develop, so production never runs a rule the repository does not have.
set -euo pipefail
BUNDLE="${1:?usage: deploy-fraud-rules.sh BUNDLE [--activate]}"
MODE="${2:---check}"
[[ "$MODE" = --check || "$MODE" = --activate ]] || exit 2
PROD="${PROD:-root@t02.golia.jp}"
HASH=$(shasum -a 256 "$BUNDLE" | awk '{print $1}')
[[ "$HASH" =~ ^[0-9a-f]{64}$ ]] || exit 2
if [ "$MODE" = --activate ]; then
    ROOT="$(dirname "$0")/.."
    COMMIT=$(git -C "$ROOT" rev-parse --verify develop)
    REPO=$(for f in brands helpers identity provenance content; do
        git -C "$ROOT" show "$COMMIT:crates/fraud-lua/rules/$f.lua"
        printf '\n'
    done | shasum -a 256 | awk '{print $1}')
    if [ "$REPO" != "$HASH" ]; then
        echo "refusing to activate lua:$HASH: develop ($COMMIT) bundles lua:$REPO." >&2
        echo 'Commit the rules to crates/fraud-lua/rules/ and merge them into develop first.' >&2
        exit 1
    fi
fi
ssh "$PROD" 'mkdir -p /apps/mailrs/fraud-rules'
scp -q "$BUNDLE" "$PROD:/apps/mailrs/fraud-rules/candidate-$HASH.lua"
ssh "$PROD" bash -s -- "$HASH" "$MODE" <<'REMOTE'
set -euo pipefail
hash="$1"; mode="$2"
cd /apps/mailrs/fraud-rules
# Serialize rule publication; compare and rename must refer to the same base.
exec 9>.publish.lock
flock -x 9
candidate="candidate-$hash.lua"
[ "$(sha256sum "$candidate" | cut -d ' ' -f1)" = "$hash" ]
baseline=builtin
[ ! -f active.lua ] || baseline=/fraud-rules/active.lua
docker exec mailrs-fastcore mailrs-fraud-check validate "/fraud-rules/$candidate"
docker exec mailrs-fastcore mailrs-fraud-check compare "$baseline" "/fraud-rules/$candidate" /data/maildir 100000 > "report-$hash.json"
# The gate's first number: each conversation as stored now against the
# candidate judged by the running core, the walk the re-judge after
# activation makes (newest message, whitelist, releases), dry. The pack
# diff above judges both packs with this binary and cannot see what a new
# binary changes. Paged: one call over every conversation outlives HTTP.
# The core secret stays in the container: printf is a shell builtin and
# curl reads its header from stdin, so it is on no command line.
live="live-$hash.jsonl"
: > "$live"
limit=2000
skip=0
for _ in $(seq 1 100); do
    docker exec mailrs-fastcore sh -c 'printf "header = \"Authorization: Bearer %s\"\n" "$MAILRS_CORE_API_SECRET" | curl -sS -f -K - -X POST "http://localhost:3301/v1/admin/maintenance:fraud-rescan?candidate=$1&skip=$2&limit=$3"' sh "$hash" "$skip" "$limit" >> "$live"
    echo >> "$live"
    if tail -n 1 "$live" | grep -q '"done":true'; then
        break
    fi
    skip=$((skip + limit))
done
# refuses pages that are not one complete dry walk with this candidate
docker exec mailrs-fastcore mailrs-fraud-check gate "/fraud-rules/report-$hash.json" "/fraud-rules/$live" > "gate-$hash.json"
cat "gate-$hash.json"
echo "--- summary (lua:$hash) ---"
docker exec mailrs-fastcore mailrs-fraud-check summary "/fraud-rules/gate-$hash.json"
if [ "$mode" = --activate ]; then
    [ ! -f active.lua ] || cp -p active.lua previous.lua
    chmod 644 "$candidate"
    mv "$candidate" active.lua
    echo "Activated lua:$hash; scoring workers reload on their next message (poll interval <=5s)."
    echo 'Historical backfill is queued automatically by fastcore; progress: /data/kevy-fastcore/fraud-backfill/progress.json'
else
    echo 'Dry run only. Review the diff, then repeat with --activate.'
fi
REMOTE
