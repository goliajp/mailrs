#!/usr/bin/env bash
# usage: ./scripts/migrate-meilisearch.sh <from-version> [--remote]
#
# meilisearch refuses to open a database written by a different engine
# version (anything other than a same-minor patch bump). this script
# automates the official dump/restore migration:
#
#   1. temporarily switch the compose file to <from-version> so the existing
#      data is readable.
#   2. trigger a dump via the HTTP API, poll until finished, copy the dump
#      file out of the container to the host.
#   3. stop meili, wipe the data volume, restore the compose file to the
#      target version (whatever the file pointed at before this script ran).
#   4. start meili with a one-shot --import-dump override that loads the
#      dump into the fresh volume at the new engine version.
#   5. confirm doc count matches, remove the override, restart normally.
#
# the dump file is preserved at the deploy dir so it can be re-imported
# manually if anything downstream goes wrong.
#
# example:
#   # currently on v1.44, but data on disk was written by v1.13:
#   ./scripts/migrate-meilisearch.sh v1.13 --remote
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <from-version> [--remote]" >&2
  exit 1
fi

FROM_VERSION="$1"
shift
REMOTE=false
for arg in "$@"; do
  case "$arg" in
    --remote) REMOTE=true ;;
    *) echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

MEILI_KEY="${MAILRS_MEILI_KEY:-mailrs-meili-key}"

if [ "$REMOTE" = true ]; then
  SSH_KEY="${SSH_KEY:-$HOME/keys/aws.pem}"
  SSH_HOST="${SSH_HOST:-root@t02.golia.jp}"
  REMOTE_DIR="${REMOTE_DIR:-/apps/mailrs}"
  SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no"
  run() { ssh $SSH_OPTS "$SSH_HOST" "cd $REMOTE_DIR && $*"; }
  echo "==> target: remote ($SSH_HOST:$REMOTE_DIR)"
else
  run() { bash -c "cd '$ROOT' && $*"; }
  echo "==> target: local ($ROOT)"
fi
echo ""

COMPOSE_FILE="docker-compose.yml"
NETWORK="$(basename "$ROOT" | tr -d -)_default"

# discover the network actually used by docker compose (handles project name overrides)
NETWORK=$(run "docker compose ps --format '{{.Networks}}' meilisearch 2>/dev/null | head -1" || true)
if [ -z "$NETWORK" ]; then
  # fall back to inspect-by-label after the service is up; first iteration just guesses
  NETWORK="mailrs_default"
fi

curl_meili() {
  local method="$1" path="$2" extra="${3:-}"
  run "docker run --rm --network $NETWORK curlimages/curl:latest \
    -s -X $method -H 'Authorization: Bearer $MEILI_KEY' $extra \
    http://meilisearch:7700$path"
}

echo "==> reading target version from $COMPOSE_FILE"
TARGET_VERSION=$(grep -E '^\s*image:\s*getmeili/meilisearch:' "$COMPOSE_FILE" | head -1 \
  | sed -E 's|.*getmeili/meilisearch:||' | tr -d ' ')
if [ -z "$TARGET_VERSION" ]; then
  echo "error: could not find getmeili/meilisearch tag in $COMPOSE_FILE" >&2
  exit 1
fi
echo "    target: $TARGET_VERSION"
echo "    source: $FROM_VERSION"
if [ "$TARGET_VERSION" = "$FROM_VERSION" ]; then
  echo "error: target and source versions are the same — nothing to migrate" >&2
  exit 1
fi
echo ""

# remote needs the from-version compose change persisted to its own copy
echo "==> switching compose to $FROM_VERSION"
run "cp $COMPOSE_FILE $COMPOSE_FILE.migrate-bak"
run "sed -i 's|getmeili/meilisearch:$TARGET_VERSION|getmeili/meilisearch:$FROM_VERSION|' $COMPOSE_FILE"
run "docker compose up -d meilisearch"

echo "==> waiting for $FROM_VERSION to become available"
for i in $(seq 1 30); do
  status=$(curl_meili GET /health 2>/dev/null || true)
  if echo "$status" | grep -q available; then
    echo "    ready"
    break
  fi
  sleep 2
done
if ! echo "$status" | grep -q available; then
  echo "error: meilisearch $FROM_VERSION did not become healthy" >&2
  run "docker compose logs --tail 30 meilisearch" >&2
  exit 1
fi

# re-resolve the network now that the service is up
NETWORK=$(run "docker inspect mailrs-meilisearch -f '{{range \$k,\$v := .NetworkSettings.Networks}}{{\$k}}{{end}}'" 2>/dev/null || echo "mailrs_default")
echo "    using docker network: $NETWORK"

echo ""
echo "==> triggering dump"
dump_resp=$(curl_meili POST /dumps)
echo "    $dump_resp"
task_uid=$(echo "$dump_resp" | python3 -c 'import json,sys;print(json.load(sys.stdin)["taskUid"])')

echo "==> waiting for dump task $task_uid"
while :; do
  task=$(curl_meili GET "/tasks/$task_uid")
  status=$(echo "$task" | python3 -c 'import json,sys;print(json.load(sys.stdin)["status"])')
  case "$status" in
    succeeded) echo "    succeeded"; break ;;
    failed|canceled) echo "error: dump task ended with status $status: $task" >&2; exit 1 ;;
    *) sleep 2 ;;
  esac
done

dump_uid=$(echo "$task" | python3 -c 'import json,sys;print(json.load(sys.stdin)["details"]["dumpUid"])')
dump_filename="${dump_uid}.dump"
host_dump_path="meili-dump-${dump_uid}.dump"
echo "==> copying $dump_filename out of container to ./$host_dump_path"
run "docker cp mailrs-meilisearch:/meili_data/dumps/$dump_filename ./$host_dump_path"
run "ls -lh $host_dump_path"

echo ""
echo "==> stopping $FROM_VERSION, wiping volume, restoring compose to $TARGET_VERSION"
run "docker compose stop meilisearch"
run "docker compose rm -f meilisearch"
run "docker volume rm mailrs_meili-data"
run "mv $COMPOSE_FILE.migrate-bak $COMPOSE_FILE"

echo "==> writing one-shot docker-compose.override.yml for --import-dump"
run "cat > docker-compose.override.yml <<YAML
services:
  meilisearch:
    command: [\"meilisearch\", \"--import-dump\", \"/import.dump\"]
    volumes:
      - ./$host_dump_path:/import.dump:ro
YAML"

echo "==> starting $TARGET_VERSION with import"
run "docker compose up -d meilisearch"

echo "==> waiting for $TARGET_VERSION to become available (import may take a while)"
for i in $(seq 1 60); do
  status=$(curl_meili GET /health 2>/dev/null || true)
  if echo "$status" | grep -q available; then
    echo "    ready"
    break
  fi
  sleep 3
done
if ! echo "$status" | grep -q available; then
  echo "error: meilisearch $TARGET_VERSION did not become healthy after import" >&2
  run "docker compose logs --tail 50 meilisearch" >&2
  echo "dump preserved at $REMOTE_DIR/$host_dump_path for manual re-import" >&2
  exit 1
fi

echo "==> verifying"
curl_meili GET /indexes/messages/stats | python3 -m json.tool || true

echo ""
echo "==> removing override and restarting cleanly"
run "rm docker-compose.override.yml"
run "docker compose up -d meilisearch"

echo "==> cleaning up dump file"
run "rm $host_dump_path"

echo ""
echo "==> migration complete: $FROM_VERSION → $TARGET_VERSION"
