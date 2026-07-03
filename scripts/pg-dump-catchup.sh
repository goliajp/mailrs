#!/usr/bin/env bash
# pg-dump-catchup.sh — incremental spg → kevy replay.
#
# Runs mailrs-pg-dump on the monolith with --since <last-successful-ts>
# and pipes into mailrs-fastcore-migrate on the fastcore container.
# Bookkeeps the cutover timestamp in a file. Idempotent: replaying an
# already-imported message is a no-op (kevy hset/zadd both are).
#
# Meant to run as a cron or systemd timer every N minutes on the
# host that has ssh access to both t01/t02:
#
#   */5 * * * * ./scripts/pg-dump-catchup.sh
#
# Env:
#   MAILRS_HOST=t02.golia.jp     (or t01 for staging)
#   MAILRS_SSH_KEY=~/keys/aws.pem
#   MAILRS_MONOLITH_CONTAINER=mailrs     (or mailrs-staging)
#   MAILRS_FASTCORE_CONTAINER=mailrs-fastcore  (or mailrs-staging-fastcore)
#   CATCHUP_STATE_FILE=~/.mailrs-catchup-since   default
#
# Exit non-zero on migrate error (some records failed to import).

set -euo pipefail

HOST="${MAILRS_HOST:-t01.golia.jp}"
KEY="${MAILRS_SSH_KEY:-$HOME/keys/aws.pem}"
MONO="${MAILRS_MONOLITH_CONTAINER:-mailrs-staging}"
FC="${MAILRS_FASTCORE_CONTAINER:-mailrs-staging-fastcore}"
STATE="${CATCHUP_STATE_FILE:-$HOME/.mailrs-catchup-since-${HOST}}"

# Prior high-water mark; default = 24h ago on first run.
if [[ -f "$STATE" ]]; then
    SINCE=$(cat "$STATE")
else
    SINCE=$(($(date +%s) - 86400))
fi
NOW=$(date +%s)

echo "[catchup] host=$HOST since=$SINCE now=$NOW"

# Stream: dump on monolith → pipe → migrate on fastcore.
# Two SSH sessions with a pipe between them; the middle jq -c passthrough
# proves the JSONL is valid, catches truncation.
count=$(ssh -i "$KEY" -o StrictHostKeyChecking=no "root@$HOST" \
    "docker exec $MONO mailrs-pg-dump --since $SINCE 2>/dev/null | wc -l")
if [[ "$count" -eq 0 ]]; then
    echo "[catchup] no new records; advancing state to $NOW"
    echo "$NOW" > "$STATE"
    exit 0
fi
echo "[catchup] streaming $count NDJSON lines..."

ssh -i "$KEY" -o StrictHostKeyChecking=no "root@$HOST" \
    "docker exec $MONO mailrs-pg-dump --since $SINCE 2>/dev/null" \
    | ssh -i "$KEY" -o StrictHostKeyChecking=no "root@$HOST" \
      "docker exec -i $FC mailrs-fastcore-migrate 2>&1"

echo "[catchup] advancing state to $NOW"
echo "$NOW" > "$STATE"
