#!/usr/bin/env bash
# One-shot backfill script for the Junk folder zset (v2.4.0 Phase 2).
#
# Copies every pre-cutover spam-categorized thread from the legacy
# `mailrs:user:<u>:threads:by_category:spam` zset into the new
# `mailrs:user:<u>:threads:junk` zset. Runs in-process via the
# fastcore container's shell so no extra network hop.
#
# Idempotent — zadd with an existing member just refreshes its score.
#
# Usage on prod (t02):
#   docker exec -it mailrs-fastcore kevy-cli SCAN 0 MATCH 'mailrs:user:*:threads:by_category:spam' COUNT 500
# then for each key returned:
#   docker exec -it mailrs-fastcore \
#     kevy-cli ZRANGEBYSCORE '<src-key>' '-inf' '+inf' WITHSCORES \
#     | while read tid && read score; do
#       docker exec -it mailrs-fastcore kevy-cli ZADD '<dst-key>' "$score" "$tid"
#     done
#
# The below wrapper does the same, driven by kevy's SCAN + a per-user
# loop. Run on prod ONCE after v2.4.0 checkpoint deploy.
set -euo pipefail

CONTAINER="${1:-mailrs-fastcore}"

# Grab every user we have a spam zset for
users=$(docker exec "$CONTAINER" kevy-cli --scan --pattern 'mailrs:user:*:threads:by_category:spam' \
  | sed -E 's|^mailrs:user:(.*):threads:by_category:spam$|\1|' \
  | sort -u)

# Also include users who have any scam-classified threads — the
# intelligence classifier can label a message "scam" (phishing) as
# well as "spam", and both feed the Junk folder per Phase 2 semantics.
users_scam=$(docker exec "$CONTAINER" kevy-cli --scan --pattern 'mailrs:user:*:threads:by_category:scam' \
  | sed -E 's|^mailrs:user:(.*):threads:by_category:scam$|\1|' \
  | sort -u)

all_users=$(printf '%s\n%s\n' "$users" "$users_scam" | sort -u)

count=0
for u in $all_users; do
  [ -z "$u" ] && continue
  src_spam="mailrs:user:${u}:threads:by_category:spam"
  src_scam="mailrs:user:${u}:threads:by_category:scam"
  dst="mailrs:user:${u}:threads:junk"
  # Union spam+scam into dst atomically. If dst already has an entry
  # (post-cutover arrivals), AGGREGATE MAX keeps the higher score.
  # `ZUNIONSTORE dst 3 dst src_spam src_scam` — 3 source keys, the
  # first being dst itself so we're really appending.
  docker exec "$CONTAINER" kevy-cli ZUNIONSTORE "$dst" 3 "$dst" "$src_spam" "$src_scam" AGGREGATE MAX >/dev/null 2>&1 || true
  count=$((count + 1))
done
echo "backfilled $count users' junk zsets from by_category:{spam,scam}"
