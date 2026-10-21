#!/usr/bin/env bash
# Sum the test results in the remote gate's output, and check the sum
# against the tests the binaries said they were running. Pass or fail is
# decided by exit status; this is what the deploy prints.
#
# Give it the gate's stdout alone. Merged with stderr, cargo's "Running ..."
# lines land in the middle of result lines, which libtest writes in pieces:
#
#   test result: ok     Running tests/request_contract_admin.rs (...)
#   . 13 passed; 0 failed; ...
#
# Status lines found mid-line are still taken out before matching, for a
# merged log read by hand, but that does not catch every split; it was 2
# short again on 2026-10-21, which the check below reported.
#
#   scripts/gate-tally.sh <stdout-log>
set -euo pipefail

perl -0pe 's/ {3,}(?:Running|Doc-tests) [^\n]*\n//g' "$1" | perl -ne '
    $run += $1 while /running (\d+) tests?\b/g;
    while (/test result: [A-Za-z]+\. (\d+) passed; (\d+) failed; (\d+) ignored/g) {
        $p += $1; $f += $2; $i += $3;
    }
    END {
        printf "    remote gate green: %d passed, %d failed\n", $p, $f;
        if ($run != $p + $f + $i) {
            printf "    !! %d tests ran but only %d results were counted (%d passed, %d failed, %d ignored)\n",
                $run, $p + $f + $i, $p, $f, $i;
        }
    }
'
