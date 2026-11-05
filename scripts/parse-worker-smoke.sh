#!/usr/bin/env bash
# parse-worker-smoke.sh — a parse worker killed in the middle of a message
# costs that message its readings, never the message.
#
# Release binaries of fastcore, the receiver and mailrs-parse-worker on a
# throwaway kevy dir and maildir. MAILRS_PARSE_WORKER_FAULTS=1 makes a
# message carrying `X-Parse-Worker-Fault: hang` hang the worker; the smoke
# then kills that worker with SIGKILL, found as a child of the process this
# script started.
#   1. The receiver takes one by SMTP: the SMTP answer is 250 and the
#      receiver logs the worker killed on it.
#   2. fastcore drains it (and a second one written straight into the spool):
#      both reach the maildir and the index, fastcore stays up, and its
#      health answer counts the failures.
# No network kevy, no docker.
#
# Usage:
#   ./scripts/parse-worker-smoke.sh

set -euo pipefail

TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/mailrs-parse-worker-smoke-$$"
CORE_PORT="${FASTCORE_PORT:-13303}"
SMTP_PORT="${SMTP_PORT:-12525}"
USER_ADDR="u@local.test"

cleanup() {
    for pid in "${RX_PID:-}" "${SVR_PID:-}"; do
        if [[ -n "$pid" ]]; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$WORK"
}
trap cleanup EXIT

if [[ "${SKIP_BUILD:-}" != 1 ]]; then
    cargo build --release -p mailrs-fastcore --bin mailrs-fastcore 2>&1 | tail -1
    cargo build --release -p mailrs-receiver --bin mailrs-receiver 2>&1 | tail -1
    cargo build --release -p mailrs-parse-worker --bin mailrs-parse-worker 2>&1 | tail -1
fi

MAILDIR="$WORK/maildir"
mkdir -p "$WORK/kevy" "$MAILDIR/local.test/u/"{new,cur,tmp} "$MAILDIR/.spool/incoming/"{new,cur,tmp}
umask 077
SECRET=$(openssl rand -hex 16)
printf 'Authorization: Bearer %s\n' "$SECRET" > "$WORK/auth.header"
printf '{"address":"%s","display_name":"U","password":"%s"}' "$USER_ADDR" "$(openssl rand -hex 16)" > "$WORK/account.json"

letter() {
    printf 'From: Billing <billing@shop.example.test>\r\nTo: %s\r\nSubject: %s\r\n' "$USER_ADDR" "$1"
    printf 'Message-ID: <%s@shop.example.test>\r\nDate: Thu, 5 Nov 2026 09:00:00 +0900\r\n' "$1"
    printf 'X-Parse-Worker-Fault: hang\r\nContent-Type: text/plain\r\n\r\nYour invoice is attached.\r\n'
}

export MAILRS_PARSE_WORKER_FAULTS=1
MAILRS_CORE_API_SECRET="$SECRET" MAILRS_KEVY_DATA_DIR="$WORK/kevy" MAILRS_MAILDIR="$MAILDIR" \
    MAILRS_FASTCORE_BIND="127.0.0.1:$CORE_PORT" MAILRS_IMAP_BIND=off MAILRS_IMAPS_BIND=off \
    MAILRS_POP3_BIND=off MAILRS_POP3S_BIND=off MAILRS_MANAGESIEVE_BIND=off \
    MAILRS_FASTCORE_SPOOL_INTERVAL_SECS=1 MAILRS_LOCAL_DOMAINS=local.test \
    ./target/release/mailrs-fastcore >>"$WORK/fastcore.log" 2>&1 &
SVR_PID=$!
for _ in $(seq 1 50); do
    curl -sf "http://127.0.0.1:$CORE_PORT/v1/healthz" >/dev/null && break
    sleep 0.2
done
curl -sf -H @"$WORK/auth.header" -H 'Content-Type: application/json' \
    --data @"$WORK/account.json" "http://127.0.0.1:$CORE_PORT/v1/admin/accounts" >/dev/null

MAILRS_CORE_API_SECRET="$SECRET" MAILRS_CORE_RPC_BASE="http://127.0.0.1:$CORE_PORT" \
    MAILRS_MAILDIR="$MAILDIR" MAILRS_KEVY_URL="mem://parse-worker-smoke" \
    MAILRS_HOSTNAME=mx.local.test MAILRS_LOCAL_DOMAINS=local.test MAILRS_ANTISPAM_ENABLED=true \
    MAILRS_PORT="$SMTP_PORT" MAILRS_SUBMISSION_PORT=0 MAILRS_SMTPS_PORT=0 \
    ./target/release/mailrs-receiver >>"$WORK/receiver.log" 2>&1 &
RX_PID=$!
for _ in $(seq 1 50); do
    nc -z 127.0.0.1 "$SMTP_PORT" 2>/dev/null && break
    sleep 0.2
done

# Kill the first parse worker of $1 that appears, 0.3 s after it does: the
# worker starts on the first message and hangs on it.
kill_worker_of() {
    local parent=$1 pid=""
    for _ in $(seq 1 100); do
        # -f: Linux truncates the process name to 15 characters, so -x
        # never matches there; -P keeps the match to $parent's children
        pid=$(pgrep -P "$parent" -f mailrs-parse-worker | head -1 || true)
        [[ -n "$pid" ]] && break
        sleep 0.02
    done
    [[ -n "$pid" ]] || return 1
    sleep 0.3
    kill -9 "$pid"
    echo "killed worker $pid of $parent"
}

# 1. through the receiver
kill_worker_of "$RX_PID" > "$WORK/rx-kill.txt" &
KILLER=$!
python3 -I - "$SMTP_PORT" "$USER_ADDR" "$(letter via-receiver)" > "$WORK/smtp.txt" <<'PY'
import smtplib, sys
port, rcpt, body = int(sys.argv[1]), sys.argv[2], sys.argv[3]
s = smtplib.SMTP("127.0.0.1", port, timeout=30)
code, _ = s.ehlo("sender.example.test")
try:
    s.sendmail("billing@shop.example.test", [rcpt], body.encode())
    print("250")
except smtplib.SMTPResponseException as e:
    print(e.smtp_code, e.smtp_error)
s.quit()
PY
wait "$KILLER" || true

# 2. straight into the spool, for the drain alone
F="1790000002.M1P1.straight"
ENVELOPE='{"reverse_path":"billing@shop.example.test","forward_paths":["u@local.test"],"is_authenticated":false,"conn_id":1,"target_folder":"INBOX","received_at":1790000002,"schema_version":1}'
kill_worker_of "$SVR_PID" > "$WORK/core-kill.txt" &
KILLER=$!
{
    printf 'X-Mailrs-Spool-Envelope: %s\r\n' "$(printf '%s' "$ENVELOPE" | base64 | tr -d '\n')"
    letter straight
} > "$MAILDIR/.spool/incoming/tmp/$F"
mv "$MAILDIR/.spool/incoming/tmp/$F" "$MAILDIR/.spool/incoming/new/$F"
wait "$KILLER" || true

# delivery takes the worker restarts and, for the second request on the
# same message, the 2 s limit
for _ in $(seq 1 60); do
    n=$(find "$MAILDIR/local.test/u/new" "$MAILDIR/local.test/u/cur" -type f | wc -l | tr -d ' ')
    [[ "$n" -ge 2 ]] && break
    sleep 0.5
done
sleep 1
health=$(curl -s "http://127.0.0.1:$CORE_PORT/v1/healthz" || true)

fail=0
check() {
    if eval "$2"; then
        printf "  PASS %s\n" "$1"
    else
        printf "  FAIL %s\n" "$1"
        fail=1
    fi
}
echo "smtp: $(cat "$WORK/smtp.txt")"
echo "receiver: $(cat "$WORK/rx-kill.txt")"
echo "fastcore: $(cat "$WORK/core-kill.txt")"
echo "healthz: $health"
check "the receiver accepted the message its worker was killed on" 'grep -q "^250" "$WORK/smtp.txt"'
check "a receiver worker was killed mid-message" 'grep -q killed "$WORK/rx-kill.txt"'
check "the receiver counted it as killed" 'grep "parse_worker_failed" "$WORK/receiver.log" | grep -q "killed"'
check "a fastcore worker was killed mid-message" 'grep -q killed "$WORK/core-kill.txt"'
check "both messages are in the maildir" '[[ "$n" -ge 2 ]]'
check "the spool is empty" '[[ -z "$(ls -A "$MAILDIR/.spool/incoming/new")" ]]'
check "nothing was quarantined" '[[ ! -d "$MAILDIR/.spool/quarantine" || -z "$(ls -A "$MAILDIR/.spool/quarantine")" ]]'
check "fastcore stayed up" 'kill -0 "$SVR_PID" 2>/dev/null'
check "the receiver stayed up" 'kill -0 "$RX_PID" 2>/dev/null'
check "health counts a killed worker" '[[ "$health" == *"\"kind\":\"killed\""* ]]'
check "health counts a restart" '[[ "$health" =~ \"worker_restarts\":[1-9] ]]'
if (( fail )); then
    echo "--- fastcore.log"
    tail -30 "$WORK/fastcore.log"
    echo "--- receiver.log"
    tail -30 "$WORK/receiver.log"
    exit 1
fi
