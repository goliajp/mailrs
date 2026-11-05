#!/usr/bin/env bash
# spool-abort-smoke.sh — a message that kills the release fastcore is
# quarantined after three deaths, and fastcore then stays up.
#
# Only a release binary shows this: release builds abort on panic, tests
# unwind. The message is crates/intake/tests/fixtures/rowspan-zero.eml,
# which html2text 0.17.1 divides by zero on while fastcore indexes it.
# fastcore is started on a throwaway kevy dir and maildir as a supervisor
# would restart it, until it stays up; then the spool's quarantine and the
# health answer are checked. No network kevy, no docker.
#
# Usage:
#   ./scripts/spool-abort-smoke.sh

set -euo pipefail

TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/mailrs-spool-abort-$$"
PORT="${FASTCORE_PORT:-13302}"
FIXTURE="crates/intake/tests/fixtures/rowspan-zero.eml"
FILE="1790000000.M1P1.rowspan-zero"

cleanup() {
    if [[ -n "${SVR_PID:-}" ]]; then
        kill "$SVR_PID" 2>/dev/null || true
        wait "$SVR_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

if [[ "${SKIP_BUILD:-}" != 1 ]]; then
    cargo build --release -p mailrs-fastcore --bin mailrs-fastcore 2>&1 | tail -3
    cargo build --release -p mailrs-parse-worker --bin mailrs-parse-worker 2>&1 | tail -3
fi

MAILDIR="$WORK/maildir"
mkdir -p "$WORK/kevy" "$MAILDIR/local.test/u/"{new,cur,tmp} "$MAILDIR/.spool/incoming/"{new,cur,tmp}
ENVELOPE='{"reverse_path":"sender@outside.example","forward_paths":["u@local.test"],"is_authenticated":false,"conn_id":1,"target_folder":"INBOX","received_at":1790000000,"schema_version":1}'
{
    printf 'X-Mailrs-Spool-Envelope: %s\r\n' "$(printf '%s' "$ENVELOPE" | base64 | tr -d '\n')"
    cat "$FIXTURE"
} > "$MAILDIR/.spool/incoming/new/$FILE"

SECRET=$(openssl rand -hex 16)
start() {
    MAILRS_CORE_API_SECRET="$SECRET" MAILRS_KEVY_DATA_DIR="$WORK/kevy" MAILRS_MAILDIR="$MAILDIR" \
        MAILRS_FASTCORE_BIND="127.0.0.1:$PORT" MAILRS_IMAP_BIND=off MAILRS_IMAPS_BIND=off \
        MAILRS_POP3_BIND=off MAILRS_POP3S_BIND=off MAILRS_MANAGESIEVE_BIND=off \
        ./target/release/mailrs-fastcore >>"$WORK/fastcore.log" 2>&1 &
    SVR_PID=$!
}

deaths=0
for run in 1 2 3 4 5; do
    start
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

fail=0
check() {
    if eval "$2"; then
        printf "  PASS %s\n" "$1"
    else
        printf "  FAIL %s\n" "$1"
        fail=1
    fi
}
health=$(curl -s "http://127.0.0.1:$PORT/v1/healthz" || true)
echo "healthz: $health"
check "died three times, then stayed up" '[[ $deaths -eq 3 && -n "${SVR_PID:-}" ]]'
check "the message is in quarantine" '[[ -f "$MAILDIR/.spool/quarantine/$FILE" ]]'
check "its record is beside it" '[[ -f "$MAILDIR/.spool/quarantine/$FILE.json" ]]'
check "it left incoming/new" '[[ ! -e "$MAILDIR/.spool/incoming/new/$FILE" ]]'
check "health counts it" '[[ "$health" == *"\"quarantined\":1"* ]]'
check "the log names it" 'grep -q "spool_quarantined" "$WORK/fastcore.log"'
if (( fail )); then
    tail -40 "$WORK/fastcore.log"
    exit 1
fi
echo "spool abort smoke passed"
