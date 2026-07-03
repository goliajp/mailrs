#!/usr/bin/env bash
# staging-fastcore-smoke.sh — verify the fastcore RPC surface returns
# kevy-backed data shape, even when empty.
#
# Walks the 4 Phase 8 routes:
#   GET /v1/healthz                              backend=kevy
#   GET /v1/readyz                               backend=kevy
#   POST /v1/users/{user}/conversations:list     items: []
#   GET /v1/users/{user}/conversations/categories categories: []
#   GET /v1/users/{user}/conversations/action-count  count: 0
#   GET /v1/users/{user}/conversations/unseen-count  count: 0
#
# A fresh fastcore has no kevy state, so the lists/counts return
# empty/zero. The point is to confirm the route is wired and the JSON
# shape is correct.
#
# Usage:
#   ./scripts/staging-fastcore-smoke.sh [base=http://127.0.0.1:3201] [user=lihao@golia.jp]

set -euo pipefail

BASE="${1:-http://127.0.0.1:3201}"
USER="${2:-lihao@golia.jp}"

pass() { printf "  \033[32mPASS\033[0m %s\n" "$1"; }
fail() { printf "  \033[31mFAIL\033[0m %s\n" "$1"; exit 1; }
step() { printf "\n\033[1m%s\033[0m\n" "$1"; }

step "1. GET /v1/healthz"
RESP=$(curl -sf "$BASE/v1/healthz")
echo "  $RESP"
[[ "$(echo "$RESP" | jq -r .backend)" == "kevy" ]] || fail "backend != kevy"
pass "/v1/healthz"

step "2. GET /v1/readyz"
RESP=$(curl -sf "$BASE/v1/readyz")
echo "  $RESP"
[[ "$(echo "$RESP" | jq -r .ready)" == "true" ]] || fail "not ready"
pass "/v1/readyz"

step "3. POST /v1/users/{user}/conversations:list"
RESP=$(curl -sf -X POST "$BASE/v1/users/$USER/conversations:list" \
    -H "Content-Type: application/json" \
    -d '{"limit":50}')
echo "  $RESP"
N=$(echo "$RESP" | jq '.items | length')
[[ "$N" =~ ^[0-9]+$ ]] || fail "items is not an array"
pass "conversations:list ($N items)"

step "4. GET /v1/users/{user}/conversations/categories"
RESP=$(curl -sf "$BASE/v1/users/$USER/conversations/categories")
echo "  $RESP"
N=$(echo "$RESP" | jq '.categories | length')
[[ "$N" =~ ^[0-9]+$ ]] || fail "categories is not an array"
pass "categories ($N rows)"

step "5. GET /v1/users/{user}/conversations/action-count"
RESP=$(curl -sf "$BASE/v1/users/$USER/conversations/action-count")
echo "  $RESP"
C=$(echo "$RESP" | jq '.count')
[[ "$C" =~ ^[0-9]+$ ]] || fail "count is not numeric"
pass "action-count = $C"

step "6. GET /v1/users/{user}/conversations/unseen-count"
RESP=$(curl -sf "$BASE/v1/users/$USER/conversations/unseen-count")
echo "  $RESP"
C=$(echo "$RESP" | jq '.count')
[[ "$C" =~ ^[0-9]+$ ]] || fail "count is not numeric"
pass "unseen-count = $C"

printf "\n\033[1;32mALL 6 PROBES PASSED\033[0m  (kevy backend reachable, JSON shapes correct)\n"
