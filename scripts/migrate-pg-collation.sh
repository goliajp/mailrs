#!/usr/bin/env bash
# usage: ./scripts/migrate-pg-collation.sh [--remote]
#
# fixes PG "collation version mismatch" warnings that appear after bumping the
# postgres docker image to a base OS with a different glibc (e.g. bookworm
# → trixie). reindexes the user data, the postgres maintenance db, and
# template1, then refreshes the recorded collation version on each.
#
# without --remote: runs against the local `docker compose` stack.
# with    --remote: SSHes into the production host (SSH_KEY / SSH_HOST env)
#                   and runs against /apps/mailrs there.
#
# REINDEX rebuilds every text index using the current libc collation, which
# is required because indexes built under the old libc may sort or compare
# incorrectly when read by code running against the new libc.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REMOTE=false
if [[ "${1:-}" == "--remote" ]]; then
  REMOTE=true
fi

DATABASES=(mailrs postgres template1)

run_psql_local() {
  local db="$1" sql="$2"
  docker compose exec -T postgres psql -U mailrs -d "$db" -c "$sql"
}

run_psql_remote() {
  local db="$1" sql="$2"
  local ssh_key="${SSH_KEY:-$HOME/keys/aws.pem}"
  local ssh_host="${SSH_HOST:-root@t02.golia.jp}"
  ssh -i "$ssh_key" -o StrictHostKeyChecking=no "$ssh_host" \
    "cd /apps/mailrs && docker compose exec -T postgres psql -U mailrs -d '$db' -c \"$sql\""
}

if [ "$REMOTE" = true ]; then
  RUN=run_psql_remote
  echo "==> target: remote (${SSH_HOST:-root@t02.golia.jp})"
else
  RUN=run_psql_local
  echo "==> target: local docker compose"
fi
echo ""

# REINDEX DATABASE cannot run inside a transaction block, so each invocation
# is one SQL statement; psql -c puts each in its own implicit transaction.
for db in "${DATABASES[@]}"; do
  echo "==> reindexing database: $db"
  $RUN "$db" "REINDEX DATABASE $db;" || {
    echo "warning: reindex of $db failed (continuing)"
    continue
  }
  echo "==> refreshing collation version: $db"
  $RUN "$db" "ALTER DATABASE $db REFRESH COLLATION VERSION;"
  echo ""
done

echo "==> verifying"
$RUN postgres "SELECT datname, datcollversion AS recorded, pg_database_collation_actual_version(oid) AS current FROM pg_database WHERE datname IN ('mailrs','postgres','template1');"

echo ""
echo "==> done"
