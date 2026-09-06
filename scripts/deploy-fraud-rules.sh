#!/usr/bin/env bash
# Bundle, review a read-only corpus diff, then activate without rebuilding.
# Usage: deploy-fraud-rules.sh BUNDLE [--activate]
# Build a bundle: cat crates/fraud-lua/rules/{brands,helpers,identity,provenance,content}.lua > /tmp/fraud.lua
set -euo pipefail
BUNDLE="${1:?usage: deploy-fraud-rules.sh BUNDLE [--activate]}"
MODE="${2:---check}"
[[ "$MODE" = --check || "$MODE" = --activate ]] || exit 2
PROD="${PROD:-root@t02.golia.jp}"
HASH=$(shasum -a 256 "$BUNDLE" | awk '{print $1}')
[[ "$HASH" =~ ^[0-9a-f]{64}$ ]] || exit 2
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
cat "report-$hash.json"
if [ "$mode" = --activate ]; then
    [ ! -f active.lua ] || cp -p active.lua previous.lua
    chmod 644 "$candidate"
    mv "$candidate" active.lua
    echo "Activated lua:$hash; scoring workers reload on their next message (poll interval <=5s)."
else
    echo 'Dry run only. Review the diff, then repeat with --activate.'
fi
REMOTE
