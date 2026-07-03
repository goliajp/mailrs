#!/usr/bin/env bash
# local-fastcore-smoke.sh — full chain dry-run on the dev laptop.
#
# Boots `mailrs-fastcore` against a fresh /tmp kevy dir, pipes the
# same sample NDJSON the staging script uses, then walks 16 routes
# expected to return non-404. No network deps, no docker, no SSH.
#
# Output: status code + body summary per probe, then per-probe
# PASS / FAIL line, exit 0 if all pass.
#
# Usage:
#   ./scripts/local-fastcore-smoke.sh

set -euo pipefail

TMPDIR="${TMPDIR:-/tmp}"
KEVY_DIR="$TMPDIR/mailrs-local-fastcore-$$"
PORT="${FASTCORE_PORT:-13301}"
USER_ADDR="${USER_ADDR:-u@local.test}"

cleanup() {
    if [[ -n "${SVR_PID:-}" ]]; then
        kill "$SVR_PID" 2>/dev/null || true
        wait "$SVR_PID" 2>/dev/null || true
    fi
    rm -rf "$KEVY_DIR"
}
trap cleanup EXIT

pass() { printf "  \033[32mPASS\033[0m %s\n" "$1"; }
fail() { printf "  \033[31mFAIL\033[0m %s\n" "$1"; FAIL=1; }
step() { printf "\n\033[1m%s\033[0m\n" "$1"; }

step "[1/4] cargo build --release fastcore + migrate"
cargo build --release -p mailrs-fastcore --bin mailrs-fastcore --bin mailrs-fastcore-migrate 2>&1 | tail -3

step "[2/4] sample NDJSON → mailrs-fastcore-migrate"
mkdir -p "$KEVY_DIR"
NDJSON=$(cat <<JSON
{"kind":"thread","user":"$USER_ADDR","row":{"thread_id":"t1","subject":"Welcome to local","senders_csv":"sys@local","count":2,"unread_count":1,"latest_date":100,"latest_preview":"hi","category":"inbox","importance_level":"normal","importance_score":0.5,"requires_action":true,"pinned":false,"archived":false,"has_action":true,"sent_count":0,"starred":false}}
{"kind":"thread","user":"$USER_ADDR","row":{"thread_id":"t2","subject":"Newsletter","senders_csv":"news@local","count":1,"unread_count":0,"latest_date":50,"latest_preview":"week","category":"bulk","importance_level":"low","importance_score":0.1,"requires_action":false,"pinned":false,"archived":false,"has_action":false,"sent_count":0,"starred":false}}
{"kind":"message","user":"$USER_ADDR","thread_id":"t1","message_id":"m1","internal_date":100,"category":"inbox","unread":true,"wire":{"id":1,"mailbox_id":1,"uid":1,"blob_ref":"","sender":"sys@local","recipients":"$USER_ADDR","subject":"Welcome to local","date":100,"internal_date":100,"size":50,"flags":0,"message_id":"m1","in_reply_to":"","thread_id":"t1","modseq":1,"user_address":"$USER_ADDR"}}
JSON
)
echo "$NDJSON" | MAILRS_KEVY_DATA_DIR="$KEVY_DIR" ./target/release/mailrs-fastcore-migrate

step "[3/4] booting fastcore on :$PORT"
MAILRS_KEVY_DATA_DIR="$KEVY_DIR" MAILRS_FASTCORE_BIND="127.0.0.1:$PORT" \
    ./target/release/mailrs-fastcore 2>/dev/null &
SVR_PID=$!
sleep 1.5

step "[4/4] probing 16 routes"
FAIL=0
BASE="http://127.0.0.1:$PORT"
probe() {
    local method="$1" path="$2" expect="$3" data="${4:-}"
    local args=(-s -o /tmp/local-fc.body -w "%{http_code}" -X "$method")
    if [[ -n "$data" ]]; then
        args+=(-H "Content-Type: application/json" -d "$data")
    fi
    local code
    code=$(curl "${args[@]}" "$BASE$path")
    if echo "$expect" | tr ' ' '\n' | grep -qx "$code"; then
        pass "$code $method $path"
    else
        fail "$code $method $path (expected $expect)"
    fi
}

probe GET    /v1/healthz                                                    200
probe GET    /v1/readyz                                                     200
probe POST   /v1/users/$USER_ADDR/conversations:list                        200 '{"limit":50}'
probe GET    /v1/users/$USER_ADDR/conversations/categories                  200
probe GET    /v1/users/$USER_ADDR/conversations/action-count                200
probe GET    /v1/users/$USER_ADDR/conversations/unseen-count                200
probe GET    /v1/users/$USER_ADDR/threads/t1/messages                       200
probe POST   /v1/users/$USER_ADDR/threads/t1/read                           204
probe POST   /v1/users/$USER_ADDR/threads/t1/pin                            204
probe POST   /v1/users/$USER_ADDR/threads/t1/unpin                          204
probe POST   /v1/users/$USER_ADDR/threads/t1/star                           204
probe POST   /v1/users/$USER_ADDR/threads/t1/unstar                         204
probe POST   /v1/users/$USER_ADDR/threads/t1/archive                        204
probe POST   /v1/users/$USER_ADDR/threads/t1/unarchive                      204
probe POST   /v1/users/$USER_ADDR/threads/t1/dismiss-action                 204
probe DELETE /v1/users/$USER_ADDR/threads/t1                                204

echo
if (( FAIL == 0 )); then
    printf "\033[1;32mALL 16 PROBES PASSED\033[0m\n"
    exit 0
else
    printf "\033[1;31m%d FAILURES\033[0m\n" "$FAIL"
    exit 1
fi
