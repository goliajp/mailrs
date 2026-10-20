#!/usr/bin/env bash
# Sum the test results in the remote gate's log. Pass or fail is decided by
# exit status; this total is what the deploy prints.
#
# The log merges cargo's stderr (its "Running ..." lines) with the test
# binaries' stdout, and the two interleave mid-line: a result line can sit
# behind half of another line, so it is matched anywhere, not at the start.
# Counting from the start once came out 5536 for a run of 5537 passes. For
# the same reason the merged log cannot say which binary a result belongs to.
#
#   scripts/gate-tally.sh <log>
set -euo pipefail

grep -oE 'test result: [A-Za-z]+\. [0-9]+ passed; [0-9]+ failed' "$1" \
    | awk '{p += $4; f += $6} END {printf "    remote gate green: %d passed, %d failed\n", p, f}'
