#!/usr/bin/env bash
# gen-sample-jsonl.sh — emit NDJSON suitable for mailrs-fastcore-migrate.
# For load-testing fastcore + sizing kevy disk + verifying the route
# matrix under realistic data volume.
#
# Usage:
#   ./scripts/gen-sample-jsonl.sh <user-email> <num-threads> [msgs-per-thread] > sample.ndjson
#   ./scripts/gen-sample-jsonl.sh u@x.com 1000 3 > /tmp/load.jsonl
#
# Categories are round-robin across personal / bulk / inbox so the
# /conversations/categories histogram is non-trivial.
#
# Each thread gets a unique numeric id (t1, t2, ...). Per-thread
# messages get message-IDs `m<thread>-<idx>`.

set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "usage: $0 <user> <threads> [msgs-per-thread=1]" >&2
    exit 2
fi

USER="$1"
N_THREADS="$2"
MSGS="${3:-1}"

CATS=(personal bulk inbox)
NOW=$(date +%s)

for ((i=1; i<=N_THREADS; i++)); do
    cat=${CATS[$((i % 3))]}
    date=$((NOW - i))
    unread=$((i % 2))           # half unread
    pinned=$((i % 17 == 0 ? 1 : 0))
    has_action=$((i % 31 == 0 ? 1 : 0))
    starred=$((i % 23 == 0 ? 1 : 0))

    printf '{"kind":"thread","user":"%s","row":{"thread_id":"t%d","subject":"Thread %d","senders_csv":"sender%d@example.com","count":%d,"unread_count":%d,"latest_date":%d,"latest_preview":"preview %d","category":"%s","importance_level":"normal","importance_score":0.5,"requires_action":false,"pinned":%s,"archived":false,"has_action":%s,"sent_count":0,"starred":%s}}\n' \
        "$USER" "$i" "$i" "$i" "$MSGS" "$unread" "$date" "$i" "$cat" \
        "$([ "$pinned" = 1 ] && echo true || echo false)" \
        "$([ "$has_action" = 1 ] && echo true || echo false)" \
        "$([ "$starred" = 1 ] && echo true || echo false)"

    for ((m=1; m<=MSGS; m++)); do
        mdate=$((date + m))
        munread=$((unread == 1 ? 1 : 0))
        printf '{"kind":"message","user":"%s","thread_id":"t%d","message_id":"m%d-%d","internal_date":%d,"category":"%s","unread":%s,"wire":{"id":%d,"mailbox_id":1,"uid":%d,"blob_ref":"","sender":"sender%d@example.com","recipients":"%s","subject":"Thread %d msg %d","date":%d,"internal_date":%d,"size":1024,"flags":0,"message_id":"m%d-%d","in_reply_to":"","thread_id":"t%d","modseq":1,"user_address":"%s"}}\n' \
            "$USER" "$i" "$i" "$m" "$mdate" "$cat" \
            "$([ "$munread" = 1 ] && echo true || echo false)" \
            "$((i * 100 + m))" "$((i * 100 + m))" "$i" "$USER" "$i" "$m" \
            "$mdate" "$mdate" "$i" "$m" "$i" "$USER"
    done
done
