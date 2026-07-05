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
#   ./scripts/migrate-aliases-embed-to-network.sh <ssh-host> [fastcore-container] [kevy-container]
# defaults:
#   fastcore-container = mailrs-fastcore  (prod)
#   kevy-container     = mailrs-kevy      (prod)
# e.g. staging: mailrs-staging-fastcore + mailrs-staging-kevy
set -eu

SSH_HOST="${1:?usage: $0 <ssh-host> [fastcore-container] [kevy-container]}"
FASTCORE_CT="${2:-mailrs-fastcore}"
KEVY_CT="${3:-mailrs-kevy}"

echo "== reading aliases from $FASTCORE_CT embedded kevy on $SSH_HOST"
ALIAS_JSON=$(ssh "$SSH_HOST" 'S=$(docker inspect '"$FASTCORE_CT"' --format "{{json .Config.Env}}" | tr "," "\n" | grep CORE_API_SECRET | cut -d= -f2 | tr -d "\"")
if [ -z "$S" ]; then
  curl -sf http://localhost:3201/v1/admin/aliases:local
else
  curl -sf -H "Authorization: Bearer $S" http://localhost:3201/v1/admin/aliases:local
fi')

COUNT=$(echo "$ALIAS_JSON" | python3 -c 'import sys,json; print(len(json.load(sys.stdin)["items"]))')
echo "   → found $COUNT alias entries"

if [ "$COUNT" = "0" ]; then
    echo "no aliases to migrate; done"
    exit 0
fi

echo "== seeding network kevy ($KEVY_CT) with those entries"
# Emit one shell line per entry that runs kevy-cli twice (SET + SADD). Piping
# a stream of commands into a single remote sh keeps this to one ssh + one
# docker exec startup per pair, versus one-per-command.
echo "$ALIAS_JSON" | python3 -c '
import sys, json, shlex
for it in json.load(sys.stdin)["items"]:
    src, tgt = it["source"], it["target"]
    key = f"mailrs:alias:{src}"
    print(f"kevy-cli SET {shlex.quote(key)} {shlex.quote(tgt)}")
    print(f"kevy-cli SADD mailrs:aliases:index {shlex.quote(src)}")
    print(f"echo \"   seeded {src} -> {tgt}\" 1>&2")
' | ssh "$SSH_HOST" "docker exec -i $KEVY_CT sh -s"

echo "== verify: sample GET on the last source"
LAST_SRC=$(echo "$ALIAS_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["items"][-1]["source"])')
ssh "$SSH_HOST" "docker exec $KEVY_CT kevy-cli GET 'mailrs:alias:$LAST_SRC'"

echo
echo "== next steps (manual, when you're ready to flip):"
echo "   1. edit the fastcore's compose file — add to $FASTCORE_CT env:"
echo "      MAILRS_ALIAS_STORE_BACKEND: network"
echo "      (MAILRS_KEVY_URL already set to the shared kevy)"
echo "   2. docker compose up -d fastcore  (graceful restart)"
echo "   3. verify alias resolve now hits network kevy:"
echo "      docker logs $FASTCORE_CT --tail 20 | grep 'alias-store backend'"
echo "      → expect: alias-store backend = network kevy"
echo "   4. embed rows can be dropped later (cleanup follow-up):"
echo "      docker exec $FASTCORE_CT rm /data/kevy-fastcore/aof-*.aof"
echo "      (but they're harmless dead weight, no rush)"
