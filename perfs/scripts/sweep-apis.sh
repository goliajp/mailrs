#!/usr/bin/env bash
# usage: TOKEN=… ./scripts/sweep-apis.sh > data/<date>/sweep.txt
# walks every API surface this report covers, 3 runs each, prints median
# row in the same shape as timing.sh.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
TIM="$HERE/timing.sh"

run3() {
  # collect 3 timings, print the line whose `total=NNNms` is the median
  local label=$1 method=$2 url=$3 data=${4:-}
  local lines=()
  for _ in 1 2 3; do
    lines+=("$("$TIM" "$label" "$method" "$url" "${data:-}")")
  done
  printf '%s\n' "${lines[@]}" \
    | sort -t= -k7 -n \
    | sed -n '2p'
}

ETID=${ETID:-$(curl -s -H "Authorization: Bearer $TOKEN" 'https://mail.golia.ai/api/conversations?limit=5' \
  | python3 -c 'import sys,json,urllib.parse; ts=json.load(sys.stdin); print(urllib.parse.quote(ts[0]["thread_id"], safe=""))')}

echo "=== /login (no auth) ==="
TOKEN= run3 "POST /api/auth/login"                        POST 'https://mail.golia.ai/api/auth/login' '{"address":"lihao@golia.jp","password":"@EscF1F2F3"}'

echo
echo "=== /dashboard ==="
run3 "GET  /api/conversations?limit=200"                  GET 'https://mail.golia.ai/api/conversations?limit=200'
run3 "GET  /api/mail/stats"                               GET 'https://mail.golia.ai/api/mail/stats'
run3 "GET  /api/mail/folders"                             GET 'https://mail.golia.ai/api/mail/folders'

echo
echo "=== /mail (chat list) — initial ==="
run3 "GET  /api/conversations?limit=50"                   GET 'https://mail.golia.ai/api/conversations?limit=50'
run3 "GET  /api/conversations/categories"                 GET 'https://mail.golia.ai/api/conversations/categories'
run3 "GET  /api/conversations/action-count"               GET 'https://mail.golia.ai/api/conversations/action-count'

echo
echo "=== /mail tab switches ==="
run3 "GET  /api/conversations?limit=50&unread=true"       GET 'https://mail.golia.ai/api/conversations?limit=50&unread=true'
run3 "GET  /api/conversations?limit=50&starred=true"      GET 'https://mail.golia.ai/api/conversations?limit=50&starred=true'
run3 "GET  /api/conversations?limit=50&folder=Sent"       GET 'https://mail.golia.ai/api/conversations?limit=50&folder=Sent'
run3 "GET  /api/conversations?limit=50&category=spam"     GET 'https://mail.golia.ai/api/conversations?limit=50&category=spam'
run3 "GET  /api/conversations?limit=50&section=action"    GET 'https://mail.golia.ai/api/conversations?limit=50&section=action'
run3 "GET  /api/conversations?limit=50&section=important" GET 'https://mail.golia.ai/api/conversations?limit=50&section=important'

echo
echo "=== /mail open thread ==="
run3 "GET  /api/conversations/<id>"                       GET "https://mail.golia.ai/api/conversations/$ETID"
run3 "GET  /api/conversations/<id>/reactions"             GET "https://mail.golia.ai/api/conversations/$ETID/reactions"

echo
echo "=== /mail search ==="
run3 "GET  /api/conversations/search?q=invoice"           GET 'https://mail.golia.ai/api/conversations/search?q=invoice&limit=50'
run3 "GET  /api/conversations/search?q=金额"               GET 'https://mail.golia.ai/api/conversations/search?q=%E9%87%91%E9%A2%9D&limit=50'

echo
echo "=== /settings ==="
run3 "GET  /api/auth/recovery-email"                      GET 'https://mail.golia.ai/api/auth/recovery-email'
run3 "GET  /api/auth/totp/status"                         GET 'https://mail.golia.ai/api/auth/totp/status'
run3 "GET  /api/mail/keys/status"                         GET 'https://mail.golia.ai/api/mail/keys/status'
run3 "GET  /api/mail/signatures"                          GET 'https://mail.golia.ai/api/mail/signatures'
run3 "GET  /api/agent/keys"                               GET 'https://mail.golia.ai/api/agent/keys'
run3 "GET  /api/agent/webhooks"                           GET 'https://mail.golia.ai/api/agent/webhooks'

echo
echo "=== /admin overview ==="
run3 "GET  /api/admin/audit/accounts"                     GET 'https://mail.golia.ai/api/admin/audit/accounts'
run3 "GET  /api/admin/audit-log?limit=10"                 GET 'https://mail.golia.ai/api/admin/audit-log?limit=10'

echo
echo "=== /admin/* listing endpoints ==="
run3 "GET  /api/admin/domains"                            GET 'https://mail.golia.ai/api/admin/domains'
run3 "GET  /api/admin/accounts"                           GET 'https://mail.golia.ai/api/admin/accounts'
run3 "GET  /api/admin/aliases"                            GET 'https://mail.golia.ai/api/admin/aliases'
run3 "GET  /api/admin/apps"                               GET 'https://mail.golia.ai/api/admin/apps'
run3 "GET  /api/admin/groups"                             GET 'https://mail.golia.ai/api/admin/groups'
run3 "GET  /api/admin/permissions"                        GET 'https://mail.golia.ai/api/admin/permissions'
run3 "GET  /api/admin/email-groups"                       GET 'https://mail.golia.ai/api/admin/email-groups'
run3 "GET  /api/queue"                                    GET 'https://mail.golia.ai/api/queue'
run3 "GET  /api/admin/audit-log?limit=200"                GET 'https://mail.golia.ai/api/admin/audit-log?limit=200'
run3 "GET  /api/admin/config/smtp"                        GET 'https://mail.golia.ai/api/admin/config/smtp'
run3 "GET  /api/health"                                   GET 'https://mail.golia.ai/api/health'
run3 "GET  /api/status"                                   GET 'https://mail.golia.ai/api/status'

echo
echo "=== misc surfaces ==="
run3 "GET  /api/contacts?q=&limit=20"                     GET 'https://mail.golia.ai/api/contacts?q=&limit=20'
run3 "GET  /api/bimi/golia.jp"                            GET 'https://mail.golia.ai/api/bimi/golia.jp'
run3 "GET  /api/conversations/batch (size 3)"             POST 'https://mail.golia.ai/api/conversations/batch' '{"thread_ids":["a","b","c"]}'
