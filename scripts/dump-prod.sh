#!/usr/bin/env bash
# usage: ./scripts/dump-prod.sh [output-path]
#
# Pulls a fresh pg_dump from prod (mail.golia.ai) for SPG cutover
# acceptance gates. Default output is /tmp/prod-data.sql.
#
# Always uses --on-conflict-do-nothing (PG 16+) so the pre-seeded
# rows from migrate-013-rbac / migrate-014-apps / migrate-029-oidc /
# migrate-030-system-config don't collide with the fresh-load
# schema-build's pre-seeds. Without this flag, round-trip loads
# produce 17 spurious PK / UNIQUE violations that have nothing
# to do with the engine under test.
#
# Required env: SSH_HOST (defaults to t02 if not set).

set -euo pipefail

OUT="${1:-/tmp/prod-data.sql}"
SSH_HOST="${SSH_HOST:-root@t02.golia.jp}"
SSH_KEY="${SSH_KEY:-$HOME/keys/aws.pem}"
REMOTE_DIR="${REMOTE_DIR:-/apps/mailrs}"
SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no"

# pg_dump flags:
#   --data-only            : schema is owned by init-schema + migrate-*.sql; don't dump it
#   --inserts              : INSERT statements (not COPY) — matches the format SPG / round-7..10 used
#   --on-conflict-do-nothing : PG 16+; avoid pre-seed dup collisions on fresh-load
#   --no-owner --no-privileges : strip role grants that mean nothing outside prod
PG_DUMP_FLAGS="--data-only --inserts --on-conflict-do-nothing --no-owner --no-privileges"

echo "==> pulling fresh pg_dump from $SSH_HOST"
ssh $SSH_OPTS "$SSH_HOST" "cd $REMOTE_DIR && docker compose exec -T postgres \
  pg_dump -U mailrs -d mailrs $PG_DUMP_FLAGS" > "$OUT"

bytes=$(wc -c < "$OUT")
mb=$((bytes / 1024 / 1024))
inserts=$(grep -cE "^INSERT INTO " "$OUT" || true)
tables=$(grep -oE "^INSERT INTO public\\.[a-z_]+" "$OUT" | sort -u | wc -l | tr -d ' ')

echo "==> dump written to $OUT"
printf "    size        : %d MB (%d bytes)\n" "$mb" "$bytes"
printf "    INSERT count: %s\n" "$inserts"
printf "    tables seen : %s\n" "$tables"
