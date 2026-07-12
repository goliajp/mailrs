#!/usr/bin/env bash
# outbound-crash-test-kill.sh — Phase 7 P8-B-B crash-inducer.
# `docker restart -t 0`s the staging sender every KILL_EVERY seconds
# for WINDOW seconds total. The `restart -t 0` variant is equivalent
# to SIGKILL + immediate start — same crash-mid-send effect as
# `docker kill` but without daemon treating it as explicit stop
# (SIGKILL alone + `restart: unless-stopped` skips the auto-restart,
# leaving the sender in `Exited (137)` forever, defeating the whole
# harness point).
#
# Usage:
#   ./scripts/outbound-crash-test-kill.sh              # kill every 10s / 60s window
#   KILL_EVERY=5 WINDOW=180 ./scripts/outbound-crash-test-kill.sh
#
# Environment:
#   STAGING_HOST  SSH host running staging (default t01)
#   CONTAINER     Sender container name (default mailrs-staging-fastcore-sender)
#   KILL_EVERY    Seconds between kills (default 10)
#   WINDOW        Total seconds to run (default 60)
#
# Run this in a separate shell alongside gen.sh so injection and
# crashes interleave. The verifier reads kevy after both finish.
set -euo pipefail

HOST="${STAGING_HOST:-t01}"
CONTAINER="${CONTAINER:-mailrs-staging-fastcore-sender}"
KILL_EVERY="${KILL_EVERY:-10}"
WINDOW="${WINDOW:-60}"

echo "==> kill loop: $CONTAINER on $HOST every ${KILL_EVERY}s for ${WINDOW}s"
END=$(( $(date +%s) + WINDOW ))
KILLS=0
while [ "$(date +%s)" -lt "$END" ]; do
    if ssh "$HOST" "docker restart -t 0 '$CONTAINER'" >/dev/null 2>&1; then
        KILLS=$(( KILLS + 1 ))
        echo "   [$(date +%H:%M:%S)] restart #$KILLS"
    else
        echo "   [$(date +%H:%M:%S)] restart failed (container missing?)"
    fi
    # Wait for the restart policy to bring the sender back up before
    # the next kill — otherwise consecutive kills stack up on a
    # container that hasn't recovered yet.
    sleep "$KILL_EVERY"
done
echo "==> kill loop done: total_kills=$KILLS"
