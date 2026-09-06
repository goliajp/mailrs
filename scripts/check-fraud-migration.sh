#!/usr/bin/env bash
# Before the first Lua roll: new image, live corpus mounted read-only, zero drift.
set -euo pipefail
PROD="${PROD:-root@t02.golia.jp}"
IMAGE="${1:?usage: check-fraud-migration.sh IMAGE}"
[[ "$IMAGE" =~ ^ghcr.io/goliajp/mailrs:[a-zA-Z0-9._-]+$ ]] || exit 2
ssh "$PROD" bash -s -- "$IMAGE" <<'REMOTE'
set -euo pipefail
image="$1"
policy=$(mktemp)
trap 'rm -f "$policy"' EXIT
docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' mailrs-receiver |
  grep -E '^MAILRS_(ORG_NAMES|LOCAL_DOMAINS|ORG_NAME_ALLOWED_DOMAINS|KEVY_URL)=' > "$policy"
docker run --rm --read-only --network container:mailrs-fastcore \
  --volumes-from mailrs-fastcore:ro --env-file "$policy" \
  --entrypoint mailrs-fraud-check "$image" compare rust builtin /data/maildir 100000
REMOTE
