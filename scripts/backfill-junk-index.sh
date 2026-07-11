#!/usr/bin/env bash
# ⚠ HISTORICAL / DEFERRED — see note below.
#
# Intent: one-shot backfill for the Junk folder zset (v2.4.0 Phase 2).
# Copies every pre-cutover spam/scam-categorized thread from
# `mailrs:user:<u>:threads:by_category:{spam,scam}` into the new
# `mailrs:user:<u>:threads:junk` zset so the Junk tab shows history
# as well as new arrivals.
#
# **Why deferred (2026-07-11 Phase 2 checkpoint deploy audit):** the
# `by_category:*` and `threads:junk` keys live in fastcore's EMBEDDED
# kevy Store (`/data/kevy-fastcore/` inside `mailrs-fastcore`), not
# in the shared network sidecar `mailrs-kevy`. `kevy-cli` ships only
# in the `mailrs-kevy` sidecar image; the fastcore image doesn't
# bundle it. So this script (which assumed `docker exec
# mailrs-fastcore kevy-cli ...`) can't run.
#
# Follow-up plan (moved to Phase 4 term sweep — see plan §4.3):
#   1. Add `crates/fastcore/src/bin/backfill_junk_index.rs` that
#      opens the embedded Store directly, iterates
#      `mailrs:user:*:threads:by_category:{spam,scam}` zsets, and
#      ZADDs each member into the corresponding `...threads:junk`
#      zset. Ship as a binary in the fastcore Docker image.
#   2. `docker exec mailrs-fastcore mailrs-fastcore-backfill-junk`
#      on prod once, post-Phase-4 deploy.
#
# Meanwhile:
#   - New arrivals from v2.4.0 checkpoint (306c0a60+) onward DO
#     populate `user_threads_junk` correctly.
#   - Historical junk stays visible via the "Spam" category tab in
#     the filter bar. Users don't lose data; the Junk tab just
#     shows a subset until the backfill runs.
#
# The script body below is preserved as reference for the intended
# ZUNIONSTORE semantics but exits early with a helpful message.
set -euo pipefail

echo "backfill-junk-index.sh is DEFERRED — see script header comment." >&2
echo "kevy-cli is not shipped inside mailrs-fastcore container." >&2
echo "Track the follow-up rust binary at plan §4.3." >&2
exit 2

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
