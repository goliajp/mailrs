#!/usr/bin/env bash
# migrate-aliases-embed-to-network.sh — one-shot cutover for RFC 20260705 Step 2.
#
# Reads every alias from a running fastcore's embedded kevy (via
# `/v1/admin/aliases:local`), writes each into the shared network kevy so
# that fastcore + pg-core + monolith all resolve against the same source
# of truth. After this + a fastcore restart with
# MAILRS_ALIAS_STORE_BACKEND=network the embed rows become dead cache.
#
# Idempotent: re-running upserts the same rows in place.
#
# Usage:
#   ./scripts/migrate-aliases-embed-to-network.sh <ssh-host>
# e.g.
#   ./scripts/migrate-aliases-embed-to-network.sh root@t02.golia.jp
set -eu

SSH_HOST="${1:?usage: $0 <ssh-host>}"

echo "== reading aliases from fastcore embedded kevy on $SSH_HOST"
ALIAS_JSON=$(ssh "$SSH_HOST" 'S=$(docker inspect mailrs-fastcore --format "{{json .Config.Env}}" | tr "," "\n" | grep CORE_API_SECRET | cut -d= -f2 | tr -d "\"")
curl -sf -H "Authorization: Bearer $S" http://localhost:3201/v1/admin/aliases:local')

COUNT=$(echo "$ALIAS_JSON" | python3 -c 'import sys,json; print(len(json.load(sys.stdin)["items"]))')
echo "   → found $COUNT alias entries"

if [ "$COUNT" = "0" ]; then
    echo "no aliases to migrate; done"
    exit 0
fi

echo "== seeding network kevy with those entries"
echo "$ALIAS_JSON" | ssh "$SSH_HOST" 'python3 - <<"PY"
import json, sys, subprocess
items = json.load(sys.stdin)["items"]
KEVY = ["docker", "exec", "-i", "mailrs-kevy", "kevy-cli"]
for it in items:
    src, tgt = it["source"], it["target"]
    key = f"mailrs:alias:{src}"
    subprocess.run(KEVY + ["SET", key, tgt], check=True, capture_output=True)
    subprocess.run(KEVY + ["SADD", "mailrs:aliases:index", src], check=True, capture_output=True)
    print(f"   seeded {src} -> {tgt}")
PY'

echo "== verify: sample GET on the last source"
LAST_SRC=$(echo "$ALIAS_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["items"][-1]["source"])')
ssh "$SSH_HOST" "docker exec mailrs-kevy kevy-cli GET 'mailrs:alias:$LAST_SRC'"

echo
echo "== next steps (manual, when you're ready to flip):"
echo "   1. edit /apps/mailrs/docker-compose.yml — add to fastcore env:"
echo "      MAILRS_ALIAS_STORE_BACKEND: network"
echo "      (MAILRS_KEVY_URL already set to the shared kevy)"
echo "   2. docker compose up -d fastcore  (graceful restart)"
echo "   3. verify alias resolve now hits network kevy:"
echo "      docker logs mailrs-fastcore --tail 20 | grep 'alias-store backend'"
echo "      → expect: alias-store backend = network kevy"
echo "   4. embed rows can be dropped later (cleanup follow-up):"
echo "      docker exec mailrs-fastcore rm /data/kevy-fastcore/aof-*.aof"
echo "      (but they're harmless dead weight, no rush)"
