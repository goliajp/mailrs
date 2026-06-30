#!/usr/bin/env bash
# staging-fastcore-import-sample.sh — pipe a tiny sample NDJSON into
# the kevy backend on t01 so the fastcore conversation list is
# non-empty for an end-to-end demo.
#
# Usage:
#   ./scripts/staging-fastcore-import-sample.sh [user-email]
#
# Requires `fastcore` service already up (see staging-fastcore-up.sh).
# Writes 3 sample threads (one per category) + 2 messages per thread.
# Then curl conversations:list to confirm.

set -euo pipefail

HOST="${STAGING_SSH_HOST:-t01.golia.jp}"
USER="${STAGING_SSH_USER:-root}"
KEY="${STAGING_SSH_KEY:-$HOME/.ssh/id_ed25519}"
EMAIL="${1:-admin@golia.jp}"
NOW=$(date +%s)

SSH="ssh -i $KEY -o StrictHostKeyChecking=no $USER@$HOST"

NDJSON=$(cat <<EOF
{"kind":"thread","user":"$EMAIL","row":{"thread_id":"sample-1","subject":"Welcome to fastcore","senders_csv":"system@golia.jp","count":2,"unread_count":2,"latest_date":$NOW,"latest_preview":"Demo thread for kevy backend","category":"inbox","importance_level":"normal","importance_score":0.5,"requires_action":false,"pinned":false,"archived":false,"has_action":false,"sent_count":0,"starred":false}}
{"kind":"thread","user":"$EMAIL","row":{"thread_id":"sample-2","subject":"Action required: review changes","senders_csv":"ci@golia.jp","count":1,"unread_count":1,"latest_date":$((NOW-3600)),"latest_preview":"Please review and approve","category":"inbox","importance_level":"high","importance_score":0.9,"requires_action":true,"pinned":false,"archived":false,"has_action":true,"sent_count":0,"starred":false}}
{"kind":"thread","user":"$EMAIL","row":{"thread_id":"sample-3","subject":"Newsletter: weekly digest","senders_csv":"newsletter@example.com","count":1,"unread_count":0,"latest_date":$((NOW-7200)),"latest_preview":"This week in tech","category":"bulk","importance_level":"low","importance_score":0.1,"requires_action":false,"pinned":false,"archived":false,"has_action":false,"sent_count":0,"starred":false}}
{"kind":"message","user":"$EMAIL","thread_id":"sample-1","message_id":"<m1-$NOW@golia.jp>","internal_date":$NOW,"category":"inbox","unread":true,"wire":{"id":1,"mailbox_id":1,"uid":1,"blob_ref":"","sender":"system@golia.jp","recipients":"$EMAIL","subject":"Welcome to fastcore","date":$NOW,"internal_date":$NOW,"size":120,"flags":0,"message_id":"m1-$NOW@golia.jp","in_reply_to":"","thread_id":"sample-1","modseq":1,"user_address":"$EMAIL"}}
{"kind":"message","user":"$EMAIL","thread_id":"sample-2","message_id":"<m2-$NOW@golia.jp>","internal_date":$((NOW-3600)),"category":"inbox","unread":true,"wire":{"id":2,"mailbox_id":1,"uid":2,"blob_ref":"","sender":"ci@golia.jp","recipients":"$EMAIL","subject":"Action required: review changes","date":$((NOW-3600)),"internal_date":$((NOW-3600)),"size":250,"flags":0,"message_id":"m2-$NOW@golia.jp","in_reply_to":"","thread_id":"sample-2","modseq":1,"user_address":"$EMAIL"}}
{"kind":"message","user":"$EMAIL","thread_id":"sample-3","message_id":"<m3-$NOW@golia.jp>","internal_date":$((NOW-7200)),"category":"bulk","unread":false,"wire":{"id":3,"mailbox_id":1,"uid":3,"blob_ref":"","sender":"newsletter@example.com","recipients":"$EMAIL","subject":"Newsletter: weekly digest","date":$((NOW-7200)),"internal_date":$((NOW-7200)),"size":2048,"flags":1,"message_id":"m3-$NOW@golia.jp","in_reply_to":"","thread_id":"sample-3","modseq":1,"user_address":"$EMAIL"}}
EOF
)

echo "[1/2] piping NDJSON into mailrs-fastcore-migrate"
echo "$NDJSON" | $SSH 'docker exec -i mailrs-staging-fastcore mailrs-fastcore-migrate'

echo
echo "[2/2] curl GET /v1/users/$EMAIL/conversations:list"
$SSH "curl -sf -X POST -H 'Content-Type: application/json' \
    -d '{\"limit\":50}' \
    http://127.0.0.1:3301/v1/users/$EMAIL/conversations:list | jq ."

cat <<EOM

done. fastcore now has 3 sample threads + 3 messages for $EMAIL.

verify counts:
  $SSH "curl -s http://127.0.0.1:3301/v1/users/$EMAIL/conversations/categories"
  $SSH "curl -s http://127.0.0.1:3301/v1/users/$EMAIL/conversations/action-count"
  $SSH "curl -s http://127.0.0.1:3301/v1/users/$EMAIL/conversations/unseen-count"

drop test data:
  $SSH 'rm -rf /var/lib/docker/volumes/mailrs-staging_mailrs-staging-data/_data/kevy-fastcore && docker restart mailrs-staging-fastcore'
EOM
