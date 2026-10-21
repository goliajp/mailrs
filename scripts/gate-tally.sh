#!/usr/bin/env bash
# Sum the test results in the remote gate's log, and check the sum against
# the tests the binaries said they were running. Pass or fail is decided by
# exit status; this is what the deploy prints.
#
# The log merges cargo's stderr with the test binaries' stdout, and libtest
# writes a result line in pieces, so cargo's "Running ..." line can land in
# the middle of one:
#
#   test result: ok     Running tests/request_contract_admin.rs (...)
#   . 13 passed; 0 failed; ...
#
# Those status lines are taken out before matching, which puts the result
# line back together. Counting without that once came out 13 short.
#
#   scripts/gate-tally.sh <log>
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
