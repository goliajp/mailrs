#!/usr/bin/env bash
# check-fn-size.sh — the 200-line function limit, as a ratchet.
#
# A function is its signature plus body, measured by scripts/fn-size.py,
# which blanks comments and literals before matching braces. Only prod code
# counts: tests/, benches/, fuzz/ and examples/ directories, *_tests.rs
# files and anything under #[cfg(test)] are left out. A file carrying a
# `CARVE-OUT:` marker in its first five lines (a route table, a dispatch
# table) is skipped whole.
#
#   * a function not in the baseline must be <= 200 lines
#   * a function in the baseline must not be LARGER than its baseline
#   * shrinking to <= 200 drops it from the baseline (the script says so)
#
# Baseline lines are `<lines> <path> <fn>`; `#` lines are comments and are
# kept by --update, which rewrites the entries after a split.
#
# Every run first measures fixtures that must and must not trip the limit
# (--self-test stops there), so a measurer that silently reports nothing
# fails the gate instead of passing it.

set -euo pipefail
cd "$(dirname "$0")/.."

LIMIT=200
BASELINE=scripts/fn-size-baseline.txt
PY=scripts/fn-size.py

sources() {
    find crates -name '*.rs' ! -path '*/target/*' \
        ! -path '*/tests/*' ! -path '*/benches/*' ! -path '*/fuzz/*' ! -path '*/examples/*' \
        ! -name '*_tests.rs' -print0
}

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

self_test() {
    local dir=$TMP_DIR
    body() { # body <lines> -> a fn spanning exactly that many lines
        echo "fn f$1() {"
        for _ in $(seq 3 "$1"); do echo "    let _ = 1;"; done
        echo "}"
    }
    body 201 > "$dir/over.rs"
    body 200 > "$dir/at.rs"
    # braces inside literals and comments must not move the end of the body
    {
        echo 'impl S {'
        echo '    fn tricky(&self) {'
        echo '        let _ = "{"; let _ = '"'}'"'; let _ = '"'\\''"';'
        echo '        let _ = r#"}}} "{" "#; let _ = br"{"; // }'
        echo '        /* } /* nested } */ { */'
        echo "        let f = |x: &'static str| { let g = || { x }; g() }; let _ = b'{';"
        echo "        fn inner<'a>(x: &'a str) -> &'a str { let _ = '\\u{7b}'; x }"
        for _ in $(seq 8 201); do echo '        let _ = 1;'; done
        echo '    }'
        echo '}'
        echo 'trait T {'
        echo '    fn required(&self);'
        echo '    fn provided(&self) {'
        for _ in $(seq 3 202); do echo '        let _ = 1;'; done
        echo '    }'
        echo '}'
        echo '#[cfg(test)]'
        echo 'mod tests {'
        body 300 | sed 's/^/    /'
        echo '}'
        echo '#[tokio::test]'
        body 300
        echo '#[cfg(test)]'
        echo 'mod helper;'
    } > "$dir/lib.rs"
    body 300 > "$dir/helper.rs"
    { echo '// CARVE-OUT: dispatch table'; body 300; } > "$dir/carved.rs"
    local got want
    got=$(python3 "$PY" "$LIMIT" "$dir/over.rs" "$dir/at.rs" "$dir/lib.rs" "$dir/helper.rs" \
        "$dir/carved.rs" | sed "s|$dir/||" | sort)
    want=$(printf '%s\n' "201 over.rs f201 1" "201 lib.rs tricky 2" "202 lib.rs provided 206" | sort)
    if [ "$got" != "$want" ]; then
        echo "!! fn-size self-test failed"
        echo "   want:"; printf '%s\n' "$want" | sed 's/^/     /'
        echo "   got:"; printf '%s\n' "$got" | sed 's/^/     /'
        exit 1
    fi
}

self_test
if [ "${1:-}" = "--self-test" ]; then echo "fn-size self-test OK"; exit 0; fi
UPDATE=0
[ "${1:-}" = "--update" ] && UPDATE=1

current=$(sources | xargs -0 python3 "$PY" "$LIMIT" | sort -rn)

report=$(printf "%s\n" "$current" | awk -v baseline="$BASELINE" '
    BEGIN {
        while ((getline line < baseline) > 0) {
            if (line ~ /^#/ || line == "") continue
            split(line, f, " "); base[f[2] " " f[3]] = f[1]
        }
    }
    NF >= 3 {
        key = $2 " " $3
        seen[key] = 1
        if (key in base) {
            if ($1 + 0 > base[key] + 0) print "grew\t  " $2 " " $3 ": " base[key] " -> " $1
        } else print "over\t  " $2 ":" $4 " " $3 ": " $1 " lines"
    }
    END { for (k in base) if (!(k in seen)) print "shrunk\t  " k }
')
section() { printf "%s\n" "$report" | awk -F'\t' -v k="$1" '$1 == k { print $2 }'; }
over=$(section over)
grew=$(section grew)
shrunk=$(section shrunk)

if [ "$UPDATE" = 1 ]; then
    {
        grep '^#' "$BASELINE" || true
        if [ -n "$current" ]; then printf "%s\n" "$current" | awk '{ print $1 " " $2 " " $3 }'; fi
    } > "$BASELINE.new"
    mv "$BASELINE.new" "$BASELINE"
    echo "baseline rewritten: $(grep -vc '^#' "$BASELINE" || true) functions over $LIMIT"
    exit 0
fi

fail=0
if [ -n "$over" ]; then
    echo "!! functions over the $LIMIT-line limit and not in the baseline:"
    printf "%s\n" "$over"
    echo "   Split them by responsibility, or mark a pure dispatch table with CARVE-OUT:."
    fail=1
fi
if [ -n "$grew" ]; then
    echo "!! functions grew past their baseline (the baseline only goes down):"
    printf "%s\n" "$grew"
    fail=1
fi
[ "$fail" = 1 ] && exit 1

remaining=$(grep -vc '^#' "$BASELINE" 2>/dev/null || true)
if [ -n "$shrunk" ]; then
    echo "function size OK — and these dropped to $LIMIT or under:"
    printf "%s\n" "$shrunk"
    echo "   Run ./scripts/check-fn-size.sh --update to retire them."
else
    echo "function size OK — ${remaining:-0} function(s) still on the baseline"
fi
