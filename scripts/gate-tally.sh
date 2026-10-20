#!/usr/bin/env bash
# Sum the test results in a cargo test log, and name any test binary that
# started but printed no result line. Its exit status still decides pass or
# fail; a binary without a result line otherwise only shows up as a total
# one or two short of the last run (5536 against 5537, 2026-10-21), with
# nothing to say which.
#
#   scripts/gate-tally.sh <log>
set -euo pipefail

awk '
    /^     Running |^   Doc-tests / { if (name != "" && !seen) silent[++n] = name; name = $0; sub(/^ +/, "", name); seen = 0 }
    /^test result: / { p += $4; f += $6; seen = 1; results++ }
    END {
        if (name != "" && !seen) silent[++n] = name
        printf "    remote gate green: %d passed, %d failed (%d binaries, %d results)\n", p, f, results + n, results
        for (i = 1; i <= n; i++) printf "    !! no result line from: %s\n", silent[i]
    }
' "$1"
