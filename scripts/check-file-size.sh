#!/usr/bin/env bash
# check-file-size.sh — the 500-line hard limit, as a ratchet.
#
# `.claude/rules/common/file-size.md` sets 500 lines per file across every
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
    find web/src \( -name '*.ts' -o -name '*.tsx' \) 2>/dev/null | sort
    # The iOS app was outside this gate until 2026-08-09, so it grew five
    # files past the limit while the rest of the repo was held to it. The
    # rule says every language; the script now looks where the rule does.
    find ios/Mailrs ios/MailrsTests ios/MailrsUITests -name '*.swift' 2>/dev/null | sort
}

# Carve-out #1 from rules/common/file-size.md: generated code is exempt,
# and the marker has to be grep-able. Both spellings are accepted — the
# rule asks for `CODEGEN:`, and generators write their own banner.
generated() {
    head -5 "$1" | grep -qiE 'CODEGEN:|auto-generated|@generated|DO NOT EDIT'
}

count() {
    case "$1" in
        *.rs) awk -f "$AWK" "$1" ;;
        *)    wc -l < "$1" | tr -d ' ' ;;
    esac
}

# bash 3.2 (macOS) has no associative arrays, so the baseline is looked up
# by grep. It is 45 lines; this is not the slow part.
baseline_of() {
    [ -f "$BASELINE" ] || return 0
    awk -v want="$1" '$2 == want { print $1; found = 1 } END { if (!found) print "" }' "$BASELINE"
}

over=""       # not in baseline, over the limit
grew=""       # in baseline, larger than it
shrunk=""     # in baseline, now under the limit
current=""

while read -r f; do
    [ -f "$f" ] || continue
    generated "$f" && continue
    n=$(count "$f")
    b=$(baseline_of "$f")

    # Everything still over the limit stays on the (rewritten) baseline,
    # whether it was there before or is being seeded now.
    [ "$n" -gt "$LIMIT" ] && current="$current$n $f\n"

    if [ -n "$b" ]; then
        if [ "$n" -gt "$b" ]; then
            grew="$grew\n  $f: $b -> $n"
        elif [ "$n" -le "$LIMIT" ]; then
            shrunk="$shrunk\n  $f ($n)"
        fi
    elif [ "$n" -gt "$LIMIT" ]; then
        over="$over\n  $f: $n"
    fi
done < <(sources)

if [ "$UPDATE" = 1 ]; then
    printf "%b" "$current" | sort -rn > "$BASELINE"
    echo "baseline rewritten: $(grep -c . "$BASELINE") files over $LIMIT"
    exit 0
fi

fail=0
if [ -n "$over" ]; then
    echo "!! over the $LIMIT-line limit and not in the baseline:"
    printf "%b\n" "$over"
    echo "   Split it, or add a carve-out per rules/common/file-size.md."
    fail=1
fi
if [ -n "$grew" ]; then
    echo "!! grew past its baseline (the baseline only goes down):"
    printf "%b\n" "$grew"
    echo "   Take the additions somewhere else, or split first."
    fail=1
fi
[ "$fail" = 1 ] && exit 1

remaining=$(grep -c . "$BASELINE" 2>/dev/null || echo 0)
if [ -n "$shrunk" ]; then
    echo "file size OK — and these dropped under $LIMIT:"
    printf "%b\n" "$shrunk"
    echo "   Run ./scripts/check-file-size.sh --update to retire them."
else
    echo "file size OK — $remaining file(s) still on the baseline"
fi
