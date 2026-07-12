#!/usr/bin/env bash
# outbound-crash-test-gen.sh — Phase 7 P8-B-B crash-test traffic
# generator. Injects N SMTP submissions into staging over a bounded
# wall-clock window; the crash-inducer runs in parallel and the
# verifier reads kevy state after both finish.
#
# Usage:
#   ./scripts/outbound-crash-test-gen.sh              # 100 msgs / 60 s
#   COUNT=500 WINDOW=180 ./scripts/outbound-crash-test-gen.sh
#
# Environment:
#   STAGING_HOST      SSH host running the staging stack (default t01)
#   SUBMISSION_PORT   Submission port on that host (default 2587)
#   COUNT             Number of messages to inject (default 100)
#   WINDOW            Seconds to spread the injection across (default 60)
#   FROM              Envelope sender (default noreply@golia.jp — auth-free
#                     path to submission; adjust if staging changes)
#   RCPT              Recipient (default sink@example.invalid — deliverability
#                     failure is intentional so sender exercises retry/fail
#                     transitions, which is exactly what the harness gates on)
#
# Output:
#   Prints the enqueued id count to stdout on completion. The verifier
#   reads it to compute the delivered+failed+bounced+pending+inflight
#   invariant.
set -euo pipefail

HOST="${STAGING_HOST:-t01}"
PORT="${SUBMISSION_PORT:-2587}"
COUNT="${COUNT:-100}"
WINDOW="${WINDOW:-60}"
FROM="${FROM:-noreply@golia.jp}"
RCPT="${RCPT:-sink@example.invalid}"
LABEL="crash-test-$(date +%s)"

if ! command -v swaks >/dev/null; then
    echo "swaks required — brew install swaks" >&2
    exit 1
fi

# Sleep between messages so the injection is spread across the window.
# For COUNT=100 / WINDOW=60 that's ~600 ms per injection.
SLEEP_MS=$(( WINDOW * 1000 / COUNT ))

echo "==> injecting $COUNT msgs into $HOST:$PORT over ${WINDOW}s (label=$LABEL)"
FAIL_COUNT=0
for i in $(seq 1 "$COUNT"); do
    if ! swaks \
        --to "$RCPT" \
        --from "$FROM" \
        --server "$HOST:$PORT" \
        --header "Subject: $LABEL-$i" \
        --header "X-Crash-Test: $LABEL" \
        --body "crash-test $i / $COUNT" \
        --hide-all >/dev/null 2>&1; then
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
    fi
    # bash-portable millisecond sleep
    if [ "$SLEEP_MS" -gt 0 ]; then
        sleep "0.${SLEEP_MS}"
    fi
done

echo "==> injection done: submitted=$COUNT swaks_failed=$FAIL_COUNT"
echo "label=$LABEL"
