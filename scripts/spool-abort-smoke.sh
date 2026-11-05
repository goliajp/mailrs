#!/usr/bin/env bash
# spool-abort-smoke.sh — a message that kills the release fastcore every
# time it is delivered is quarantined after three deaths, fastcore then
# stays up, and moving the file back delivers it once.
#
# Only a release binary shows this: release builds abort on panic, tests
# unwind. No parser is relied on to crash (they run in the parse worker
# now, and the one that used to crash was fixed): the smoke builds its
# own fastcore with the `abort-hook` feature, in its own target directory,
# which aborts after the maildir write and before the index when
# MAILRS_TEST_ABORT=1 and the message carries `X-Mailrs-Test-Abort: yes`.
# It then checks that the hook's marker is in that binary and not in the
# release binary built the way the image builds it.
#
# fastcore runs on a throwaway kevy dir and maildir, started again the way
# a supervisor would until it stays up. No network kevy, no docker.
#
# Usage:
#   ./scripts/spool-abort-smoke.sh

set -euo pipefail

TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/mailrs-spool-abort-$$"
PORT="${FASTCORE_PORT:-13302}"
FILE="1790000000.M1P1.abort-marked"
HOOK_TARGET="target/abort-hook"
HOOKED="$HOOK_TARGET/release/mailrs-fastcore"
PROD="target/release/mailrs-fastcore"
USER_ADDR="u@local.test"

cleanup() {
    if [[ -n "${SVR_PID:-}" ]]; then
        kill "$SVR_PID" 2>/dev/null || true
        wait "$SVR_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

if [[ "${SKIP_BUILD:-}" != 1 ]]; then
    CARGO_TARGET_DIR="$HOOK_TARGET" cargo build --release -p mailrs-fastcore \
        --features abort-hook --bin mailrs-fastcore 2>&1 | tail -1
    CARGO_TARGET_DIR="$HOOK_TARGET" cargo build --release -p mailrs-parse-worker \
        --bin mailrs-parse-worker 2>&1 | tail -1
    cargo build --release -p mailrs-fastcore --bin mailrs-fastcore 2>&1 | tail -1
fi

MAILDIR="$WORK/maildir"
mkdir -p "$WORK/kevy" "$MAILDIR/local.test/u/"{new,cur,tmp} "$MAILDIR/.spool/incoming/"{new,cur,tmp}
ENVELOPE='{"reverse_path":"sender@outside.example","forward_paths":["u@local.test"],"is_authenticated":false,"conn_id":1,"target_folder":"INBOX","received_at":1790000000,"schema_version":1}'
{
    printf 'X-Mailrs-Spool-Envelope: %s\r\n' "$(printf '%s' "$ENVELOPE" | base64 | tr -d '\n')"
    printf 'From: Sender <sender@outside.example>\r\nTo: %s\r\nSubject: killer\r\n' "$USER_ADDR"
    printf 'Message-ID: <abort-marked@outside.example>\r\nDate: Wed, 5 Nov 2026 09:00:00 +0900\r\n'
    printf 'X-Mailrs-Test-Abort: yes\r\n\r\nthis message kills the process that delivers it\r\n'
} > "$MAILDIR/.spool/incoming/new/$FILE"

umask 077
SECRET=$(openssl rand -hex 16)
printf 'Authorization: Bearer %s\n' "$SECRET" > "$WORK/auth.header"
start() {
    MAILRS_CORE_API_SECRET="$SECRET" MAILRS_KEVY_DATA_DIR="$WORK/kevy" MAILRS_MAILDIR="$MAILDIR" \
        MAILRS_FASTCORE_BIND="127.0.0.1:$PORT" MAILRS_IMAP_BIND=off MAILRS_IMAPS_BIND=off \
        MAILRS_POP3_BIND=off MAILRS_POP3S_BIND=off MAILRS_MANAGESIEVE_BIND=off \
        MAILRS_FASTCORE_SPOOL_INTERVAL_SECS=1 MAILRS_LOCAL_DOMAINS=local.test \
        "$@" "./$HOOKED" >>"$WORK/fastcore.log" 2>&1 &
    SVR_PID=$!
}

# 1. the abort is on: it dies on the message until the quarantine takes it
deaths=0
for run in 1 2 3 4 5; do
    start env MAILRS_TEST_ABORT=1
    # up for ten seconds counts as up
    for _ in $(seq 1 20); do
        kill -0 "$SVR_PID" 2>/dev/null || break
        sleep 0.5
    done
    if kill -0 "$SVR_PID" 2>/dev/null; then
        echo "run $run: fastcore is up"
        break
    fi
    code=0
    wait "$SVR_PID" || code=$?
    SVR_PID=
    deaths=$((deaths + 1))
    echo "run $run: fastcore exited with $code"
done
health1=$(curl -s "http://127.0.0.1:$PORT/v1/healthz" || true)
quarantined_file=0
[[ -f "$MAILDIR/.spool/quarantine/$FILE" ]] && quarantined_file=1
record=0
[[ -f "$MAILDIR/.spool/quarantine/$FILE.json" ]] && record=1
left_new=1
[[ -e "$MAILDIR/.spool/incoming/new/$FILE" ]] || left_new=0

# 2. fixed (the abort is off), the file is moved back as an operator would
if [[ -n "${SVR_PID:-}" ]]; then
    kill "$SVR_PID"
    wait "$SVR_PID" 2>/dev/null || true
    SVR_PID=
fi
echo "--- restarted without the abort" >>"$WORK/fastcore.log"
start env
for _ in $(seq 1 50); do
    curl -sf "http://127.0.0.1:$PORT/v1/healthz" >/dev/null && break
    sleep 0.2
done
mv "$MAILDIR/.spool/quarantine/$FILE" "$MAILDIR/.spool/incoming/new/$FILE"
for _ in $(seq 1 40); do
    [[ -e "$MAILDIR/.spool/incoming/new/$FILE" ]] || break
    sleep 0.25
done
sleep 1
health2=$(curl -s "http://127.0.0.1:$PORT/v1/healthz" || true)
copies=$(find "$MAILDIR/local.test/u/new" "$MAILDIR/local.test/u/cur" -name "$FILE*" | wc -l | tr -d ' ')
unseen=$(curl -s -H @"$WORK/auth.header" \
    "http://127.0.0.1:$PORT/v1/users/$USER_ADDR/conversations/unseen-count" || true)

fail=0
check() {
    if eval "$2"; then
        printf "  PASS %s\n" "$1"
    else
        printf "  FAIL %s\n" "$1"
        fail=1
    fi
}
echo "healthz after the deaths: $health1"
echo "healthz after moving it back: $health2"
echo "unseen count: $unseen"
check "died three times, then stayed up" '[[ $deaths -eq 3 ]]'
check "the message is in quarantine" '[[ $quarantined_file -eq 1 ]]'
check "its record is beside it" '[[ $record -eq 1 ]]'
check "it left incoming/new" '[[ $left_new -eq 0 ]]'
check "health counts it" '[[ "$health1" == *"\"quarantined\":1"* ]]'
check "the log names it" 'grep -q "spool_quarantined" "$WORK/fastcore.log"'
check "moved back, it left the spool" '[[ ! -e "$MAILDIR/.spool/incoming/new/$FILE" ]]'
check "moved back, nothing is in quarantine" '[[ "$health2" == *"\"quarantined\":0"* ]]'
check "the maildir holds exactly one copy" '[[ "$copies" -eq 1 ]]'
check "the copy written before the deaths was kept, not written again" \
    'grep -q "maildir copy already written by an earlier attempt" "$WORK/fastcore.log"'
check "it is indexed once: one unread conversation" '[[ "$unseen" == *"\"count\":1"* ]]'
check "fastcore stayed up" 'kill -0 "$SVR_PID" 2>/dev/null'
# the hook is in the smoke's binary only: the probe has to find it there
# for its absence from the release binary to mean anything. It looks for
# the switch's name, which the hook hands to the environment lookup as a
# string; the marker itself is compared in place and leaves no string.
check "the smoke binary carries the abort hook" 'grep -aq "MAILRS_TEST_ABORT" "$HOOKED"'
check "the release binary does not" '! grep -aq "MAILRS_TEST_ABORT" "$PROD"'
hook_in_tree() {
    cargo tree -p mailrs-fastcore -e features -i mailrs-intake --prefix none "$@" 2>/dev/null |
        grep -c 'feature "abort-hook"' || true
}
check "the feature tree shows the hook when asked for" '[[ "$(hook_in_tree --features abort-hook)" -gt 0 ]]'
check "and not in the build the image makes" '[[ "$(hook_in_tree)" -eq 0 ]]'
if (( fail )); then
    tail -40 "$WORK/fastcore.log"
    exit 1
fi
