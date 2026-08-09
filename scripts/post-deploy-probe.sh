#!/usr/bin/env bash
# post-deploy-probe.sh — assert the routes are actually there after a roll.
#
# The version gate answers "is the new binary running". It cannot answer "does
# it still serve the endpoints", and that is the failure this tree keeps
# shipping: the three AI routes answered 405 in production for as long as the
# fastcore lane has been the deployed one, and PUT/DELETE on system-config
# and PUT on group permissions did the same, each while every health check
# read green.
#
# What makes this checkable is that an authenticated route answers **401**
# before it looks at the method — so 401 means "this path is served". The
# two ways a route can be gone are both distinguishable:
#
#   GET  a missing path → 200 text/html   (the SPA fallback serves index)
#   POST a missing path → 405             (the fallback is GET-only)
#
# So a 200 or a 405 where 401 was expected is a missing route, and it is
# exactly what nothing caught before. Measured on prod 2026-07-31.
#
# Usage: post-deploy-probe.sh [host:port]     (default localhost:3103 via ssh)
set -euo pipefail
cd "$(dirname "$0")/.."

PROD="${PROD:-root@t02.golia.jp}"
BASE="${1:-localhost:3103}"

# METHOD path expected — one per line.
#
# Keep this to routes whose absence is user-visible. It is a smoke test, not
# a second copy of the router; check-core-contract.sh is what compares the full
# tables. Every entry below is a route that has broken, or the reads the
# mailbox cannot work without.
#
# The last four are the login page's dependencies, and they are here with
# their *unconfigured* statuses rather than 401 on purpose. For everything
# above, 401 proves the path is served. For a route that runs before anyone
# has signed in, 401 is the failure — and it is what all three sign-in
# starts answered from the fastcore cutover until 2026-08-01, because they
# were registered in the authenticated router. Nothing noticed: the probe
# only knew how to expect 401, and the login page's `.catch` turned the
# empty 401 body into "no providers configured".
#
# So: 200 where the page needs data, and 404 / 501 where an unconfigured
# provider is the honest answer. Any of them turning 401 means a sign-in
# route has been moved behind auth again.
PROBES=$(cat <<'TABLE'
GET  /api/health                                200
GET  /.well-known/autoconfig/mail/config-v1.1.xml 200
GET  /mail/config-v1.1.xml                      200
GET  /api/conversations                         401
GET  /api/conversations/categories              401
GET  /api/conversations/unseen-count            401
POST /api/conversations/mark-all-read           401
GET  /api/mail/drafts                           401
POST /api/mail/drafts                           401
GET  /api/mail/sends                            401
GET  /api/mail/folders                          401
POST /api/mail/ai/polish                        401
POST /api/mail/ai/reply-suggest                 401
POST /api/mail/ai/generate-subject              401
GET  /api/calendar/feeds                        401
POST /api/calendar/feeds                        401
GET  /api/agent/webhooks                        401
POST /api/agent/webhooks                        401
GET  /api/admin/accounts                        401
GET  /api/admin/system-config                   401
PUT  /api/admin/system-config/probe             401
DELETE /api/admin/system-config/probe           401
PUT  /api/admin/groups/1/permissions            401
GET  /api/auth/oidc/config                      200
GET  /api/auth/external-providers               200
GET  /api/auth/external/probe-not-a-provider    404
GET  /api/auth/oidc/login                       501
TABLE
)

# `ssh -n`: without it ssh reads the loop's stdin and swallows the rest of
# the table. The first version of this script probed one route out of
# twenty-three and reported "1/1 as expected" — a check that silently does
# almost nothing and calls it success, which is the class of defect this
# script exists to catch.
run() {
    if [ "$BASE" = "localhost:3103" ]; then
        ssh -n "$PROD" "curl -s -m10 -o /dev/null -w '%{http_code}' -X $1 -H 'content-type: application/json' -d '{}' 'http://$BASE$2'"
    else
        curl -s -m10 -o /dev/null -w '%{http_code}' -X "$1" -H 'content-type: application/json' -d '{}' "http://$BASE$2"
    fi
}

expected_count=$(printf '%s\n' "$PROBES" | grep -c .)
fails=0
checked=0
while IFS= read -r line; do
    [ -n "$line" ] || continue
    method=$(echo "$line" | awk '{print $1}')
    path=$(echo "$line" | awk '{print $2}')
    want=$(echo "$line" | awk '{print $3}')
    got=$(run "$method" "$path" || echo 000)
    checked=$((checked + 1))
    if [ "$got" != "$want" ]; then
        printf '    !! %-6s %-48s want %s got %s\n' "$method" "$path" "$want" "$got"
        fails=$((fails + 1))
    fi
done <<< "$PROBES"

# The count is asserted, not just printed: a loop that stops early would
# otherwise report every route it did reach as a pass.
if [ "$checked" -ne "$expected_count" ]; then
    echo "!! probed $checked of $expected_count routes — the loop stopped early."
    echo "!! Something consumed stdin (ssh without -n does this)."
    exit 1
fi

if [ "$fails" -eq 0 ]; then
    echo "    route probe: $checked/$expected_count as expected"
    exit 0
fi

echo
echo "!! $fails of $checked probed routes answered something else."
echo "!! 401 means the path is served (auth runs before method routing)."
echo "!! 200 on an /api path means it fell through to the SPA fallback,"
echo "!! and 405 on a POST means there is no handler — both mean the route"
echo "!! is gone. A client hitting it gets HTML where it expected JSON."
exit 1
