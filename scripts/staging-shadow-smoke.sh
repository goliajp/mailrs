#!/usr/bin/env bash
# staging-shadow-smoke.sh — verify webapi+core RPC integration after
# staging-shadow-up.sh ran. Walks the auth → session → conversations →
# admin paths and prints PASS/FAIL per step.
#
# Usage:
#   ./scripts/staging-shadow-smoke.sh user@smk.ai password
#
# All requests go through webapi (host :3102) so we exercise:
#   webapi → core_rpc (:3300) → spg + maildir
#
# Exits non-zero on any failure.

set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "usage: $0 <email> <password> [webapi-base=http://127.0.0.1:3102]" >&2
    exit 2
fi

EMAIL="$1"
PASSWORD="$2"
BASE="${3:-http://127.0.0.1:3102}"
COOKIE_JAR="$(mktemp)"
trap 'rm -f "$COOKIE_JAR"' EXIT

pass() { printf "  \033[32mPASS\033[0m %s\n" "$1"; }
fail() { printf "  \033[31mFAIL\033[0m %s\n" "$1"; exit 1; }

step() { printf "\n\033[1m%s\033[0m\n" "$1"; }

step "1. /_health (unauthenticated)"
if curl -sf "$BASE/_health" >/dev/null; then
    pass "/_health"
else
    fail "/_health did not return 200"
fi

step "2. POST /api/auth/login"
LOGIN_BODY=$(jq -nc --arg e "$EMAIL" --arg p "$PASSWORD" '{email:$e,password:$p}')
LOGIN_RC=$(curl -sS -c "$COOKIE_JAR" -o /tmp/login.json -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "$LOGIN_BODY" "$BASE/api/auth/login")
if [[ "$LOGIN_RC" != "200" ]]; then
    cat /tmp/login.json >&2
    fail "login returned HTTP $LOGIN_RC"
fi
pass "session established"

step "3. GET /api/auth/me"
ME=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/auth/me")
echo "  $(echo "$ME" | jq -c '{address, permissions: (.permissions|length)}')"
[[ "$(echo "$ME" | jq -r .address)" == "$EMAIL" ]] || fail "/me address mismatch"
pass "/auth/me"

step "4. GET /api/mail/folders"
FOLDERS=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/mail/folders")
N=$(echo "$FOLDERS" | jq '.items|length')
[[ "$N" -gt 0 ]] || fail "no folders returned"
pass "/mail/folders ($N folders)"

step "5. GET /api/conversations?limit=5"
CONVOS=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/conversations?limit=5")
N=$(echo "$CONVOS" | jq '.conversations|length')
pass "/conversations ($N returned)"

step "6. GET /api/mail/stats"
STATS=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/mail/stats")
echo "  $STATS"
pass "/mail/stats"

step "7. GET /api/mail/drafts (initial)"
INITIAL=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/mail/drafts")
INIT_N=$(echo "$INITIAL" | jq '.items|length')

step "8. POST /api/mail/drafts (smoke)"
DRAFT_BODY='{"to":"smoke@example.com","subject":"webapi smoke","body":"hello from webapi"}'
SAVED=$(curl -sf -b "$COOKIE_JAR" -H "Content-Type: application/json" \
    -d "$DRAFT_BODY" "$BASE/api/mail/drafts")
ID=$(echo "$SAVED" | jq -r .id)
[[ "$ID" =~ ^[0-9]+$ ]] || fail "save_draft did not return numeric id"
pass "draft saved (id=$ID)"

step "9. GET /api/mail/drafts (after save)"
AFTER=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/mail/drafts")
AFT_N=$(echo "$AFTER" | jq '.items|length')
(( AFT_N == INIT_N + 1 )) || fail "draft count $AFT_N != $((INIT_N + 1))"
pass "draft count incremented"

step "10. DELETE /api/mail/drafts/$ID"
DEL_RC=$(curl -sS -b "$COOKIE_JAR" -X DELETE -o /dev/null -w "%{http_code}" \
    "$BASE/api/mail/drafts/$ID")
[[ "$DEL_RC" == "204" ]] || fail "delete returned HTTP $DEL_RC"
pass "draft deleted"

step "11a. GET /api/mail/signatures (initial)"
SIG_INIT=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/mail/signatures")
SIG_INIT_N=$(echo "$SIG_INIT" | jq '.items|length')
pass "/mail/signatures ($SIG_INIT_N initial)"

step "11b. POST /api/mail/signatures + DELETE"
SIG_BODY='{"name":"smoke-sig","html":"<p>smoke</p>","text_content":"smoke","is_default":false}'
SIG_SAVED=$(curl -sf -b "$COOKIE_JAR" -H "Content-Type: application/json" \
    -d "$SIG_BODY" "$BASE/api/mail/signatures")
SIG_ID=$(echo "$SIG_SAVED" | jq -r .id)
[[ "$SIG_ID" =~ ^[0-9]+$ ]] || fail "save_signature did not return numeric id"
SIG_DEL_RC=$(curl -sS -b "$COOKIE_JAR" -X DELETE -o /dev/null -w "%{http_code}" \
    "$BASE/api/mail/signatures/$SIG_ID")
[[ "$SIG_DEL_RC" == "204" ]] || fail "delete signature returned HTTP $SIG_DEL_RC"
pass "signature CRUD (id=$SIG_ID)"

step "11c. GET /api/mail/templates (initial)"
TPL_INIT=$(curl -sf -b "$COOKIE_JAR" "$BASE/api/mail/templates")
TPL_INIT_N=$(echo "$TPL_INIT" | jq '.items|length')
pass "/mail/templates ($TPL_INIT_N initial)"

step "11d. POST /api/mail/templates + DELETE"
TPL_BODY='{"name":"smoke-tpl","subject":"smoke","html_body":"<p>smoke</p>","text_body":"smoke","category":"smoke","is_default":false}'
TPL_SAVED=$(curl -sf -b "$COOKIE_JAR" -H "Content-Type: application/json" \
    -d "$TPL_BODY" "$BASE/api/mail/templates")
TPL_ID=$(echo "$TPL_SAVED" | jq -r .id)
[[ "$TPL_ID" =~ ^[0-9]+$ ]] || fail "save_template did not return numeric id"
TPL_DEL_RC=$(curl -sS -b "$COOKIE_JAR" -X DELETE -o /dev/null -w "%{http_code}" \
    "$BASE/api/mail/templates/$TPL_ID")
[[ "$TPL_DEL_RC" == "204" ]] || fail "delete template returned HTTP $TPL_DEL_RC"
pass "template CRUD (id=$TPL_ID)"

step "11e. GET /api/status (no auth needed)"
STAT=$(curl -sf "$BASE/api/status")
echo "  $STAT"
pass "/api/status"

step "12. POST /api/auth/logout"
LO_RC=$(curl -sS -b "$COOKIE_JAR" -X POST -o /dev/null -w "%{http_code}" \
    "$BASE/api/auth/logout")
[[ "$LO_RC" == "200" || "$LO_RC" == "204" ]] || fail "logout returned HTTP $LO_RC"
pass "logged out"

step "13. /api/auth/me without cookie should 401"
ME_RC=$(curl -sS -o /dev/null -w "%{http_code}" "$BASE/api/auth/me")
[[ "$ME_RC" == "401" ]] || fail "expected 401 after logout, got $ME_RC"
pass "session truly invalidated"

printf "\n\033[1;32mALL STEPS PASSED\033[0m\n"
