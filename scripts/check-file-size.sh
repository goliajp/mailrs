#!/usr/bin/env bash
# check-file-size.sh — the 500-line hard limit, as a ratchet.
#
# The house rule sets 500 lines per file across every
# language. mailrs had 45 files over it and no gate, because the copy of
# that rule it carried until 2026-08-02 listed *torajs*'s debt table —
# fourteen paths that do not exist here — so this repo's own overruns were
# never written down.
#
# Turning the limit on outright would reject every deploy. So this is a
# ratchet instead:
#
#   * a file not in the baseline must be <= 500 prod lines
#   * a file in the baseline must not be LARGER than its baseline
#   * shrinking below 500 drops it from the baseline (the script says so)
#
# The baseline can only go down. Run with --update after a split to
# rewrite it.
#
# Rust `.rs` counts prod lines only: a trailing `#[cfg(test)] mod tests`
# block is excluded (scripts/file-size.awk).

set -euo pipefail
cd "$(dirname "$0")/.."

LIMIT=500
BASELINE=scripts/file-size-baseline.txt
AWK=scripts/file-size.awk
UPDATE=0
[ "${1:-}" = "--update" ] && UPDATE=1

sources() {
    find crates -name '*.rs' ! -path '*/target/*' | sort
    { find web/src \( -name '*.ts' -o -name '*.tsx' \) 2>/dev/null || true; } | sort
    # The iOS app was outside this gate until 2026-08-09, so it grew five
    # files past the limit while the rest of the repo was held to it. The
    # rule says every language; the script now looks where the rule does.
    # A missing directory means no files there, not a failed check —
    # under pipefail a bare find would end the list here and skip the
    # Android files below without a word.
    { find ios/Mailrs ios/MailrsTests ios/MailrsUITests -name '*.swift' 2>/dev/null || true; } | sort
    # And the Android app, for the same reason and by the same argument:
    # it was outside this gate while it was written, and grew a
    # 1,460-line view model on the day the rest of the repo had none over
    # 500. The rule says every language.
    { find android/app/src -name '*.kt' 2>/dev/null || true; } | sort
}

# Carve-out #1: generated code is exempt, and the marker has to be
# grep-able. Both spellings are accepted — the rule asks for `CODEGEN:`,
# and generators write their own banner. The Rust counter reports it
# itself; the others are checked here.
#
# Every file is counted in one pass per language. A few processes per
# file used to cost about twenty seconds, nearly all of it fork and exec.
counts() {
    sources | grep '\.rs$' | tr '\n' '\0' | xargs -0 awk -f "$AWK"
    sources | grep -v '\.rs$' | tr '\n' '\0' | xargs -0 awk '
        FNR <= 5 && tolower($0) ~ /codegen:|auto-generated|@generated|do not edit/ { print FILENAME }
    ' | sort -u > "$TMP_GEN"
    sources | grep -v '\.rs$' | tr '\n' '\0' | xargs -0 wc -l \
        | awk -v gen="$TMP_GEN" '
            BEGIN { while ((getline g < gen) > 0) skip[g] = 1 }
            { n = $1; sub(/^[[:space:]]*[0-9]+[[:space:]]+/, ""); if ($0 != "total" && !($0 in skip)) print n " " $0 }
        '
}

TMP_GEN=$(mktemp)
trap 'rm -f "$TMP_GEN"' EXIT

# Everything still over the limit stays on the (rewritten) baseline,
# whether it was there before or is being seeded now.
report=$(counts | awk -v limit="$LIMIT" -v baseline="$BASELINE" '
    BEGIN { while ((getline line < baseline) > 0) { split(line, f, " "); base[f[2]] = f[1] } }
    $1 == "GEN" { next }
    {
        n = $1 + 0; path = $0; sub(/^[0-9]+ /, "", path)
        if (n > limit) print "current\t" n " " path
        if (path in base) {
            if (n > base[path] + 0) print "grew\t  " path ": " base[path] " -> " n
            else if (n <= limit) print "shrunk\t  " path " (" n ")"
        } else if (n > limit) print "over\t  " path ": " n
    }
')
section() { printf "%s\n" "$report" | awk -F'\t' -v k="$1" '$1 == k { print $2 }'; }
current=$(section current)
over=$(section over)
grew=$(section grew)
shrunk=$(section shrunk)

if [ "$UPDATE" = 1 ]; then
    if [ -n "$current" ]; then printf "%s\n" "$current"; fi | sort -rn > "$BASELINE"
    echo "baseline rewritten: $(grep -c . "$BASELINE") files over $LIMIT"
    exit 0
fi

fail=0
if [ -n "$over" ]; then
    echo "!! over the $LIMIT-line limit and not in the baseline:"
    printf "%s\n" "$over"
    echo "   Split it, or add a CODEGEN: / CARVE-OUT: marker."
    fail=1
fi
if [ -n "$grew" ]; then
    echo "!! grew past its baseline (the baseline only goes down):"
    printf "%s\n" "$grew"
    echo "   Take the additions somewhere else, or split first."
    fail=1
fi
[ "$fail" = 1 ] && exit 1

remaining=$(grep -c . "$BASELINE" 2>/dev/null || echo 0)
if [ -n "$shrunk" ]; then
    echo "file size OK — and these dropped under $LIMIT:"
    printf "%s\n" "$shrunk"
    echo "   Run ./scripts/check-file-size.sh --update to retire them."
else
    echo "file size OK — $remaining file(s) still on the baseline"
fi
