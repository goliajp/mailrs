#!/usr/bin/env bash
# check-rest-parity.sh — the two REST lanes must agree on what they serve.
#
# mailrs has two web implementations: `crates/webapi` (kevy, what production
# runs) and `crates/server/src/web` (spg SQL, the dormant dogfood lane). One
# client talks to whichever is deployed, so a route that exists on one and
# not the other is a feature that works or 405s depending on where it lands
# — which is how Polish, Suggest and Generate subject answered 405 in
# production for as long as the fastcore lane has been the deployed one.
#
# This is the REST counterpart of check-mcp-parity.sh. It reports every
# `METHOD /path` served by one lane and not the other.
#
# A difference fails unless `.claude/rest-parity-allow.txt` names it with a
# reason. The allow-list is printed on every run, so it cannot grow into a
# place where things are quietly parked.
#
# Standing decision (feedback-fastcore-core-mode-parity, 2026-07-22): shared
# behaviour belongs in a stone, and the dormant lane does not get
# hand-written duplicate glue. An allow-list entry is therefore the expected
# outcome for a route the dormant lane has no backing for. What this gate
# prevents is the *unnoticed* divergence.
#
# Exit 0 = every difference is either absent or accounted for.
set -euo pipefail
# Byte order everywhere. These strings are full of `/`, `{` and `.`, which
# most locales collate differently from byte order — and `comm` compares in
# the locale's order, so a locale-sorted pair reads as two disjoint sets.
export LC_ALL=C
cd "$(dirname "$0")/.."

ALLOW=".claude/rest-parity-allow.txt"

extract() {
    python3 - "$@" <<'PYEOF'
import re, sys

# `.route("<path>", get(h).post(h2))` — take the path, then every method
# named in the handler expression. Axum chains methods, so one entry can
# accept several, and the call is often written across three or four lines;
# the expression is found by matching parentheses rather than by reading to
# the end of a line.
METHODS = ("get", "post", "put", "patch", "delete", "head", "options")


def expression(src, open_paren):
    depth = 0
    i = open_paren
    while i < len(src):
        c = src[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return src[open_paren + 1:i]
        i += 1
    return ""


out = set()
for arg in sys.argv[1:]:
    try:
        src = open(arg).read()
    except OSError:
        continue
    for m in re.finditer(r"\.route\(", src):
        body = expression(src, m.end() - 1)
        # A `//` comment between `.route(` and the path is ordinary and must
        # not hide the route — leaving one out of the comparison is exactly
        # the silence this gate exists to remove.
        stripped = re.sub(r"^(?:\s*//[^\n]*\n)+", "", body)
        route = re.match(r'\s*"([^"]+)"\s*,', stripped)
        if not route:
            continue
        handler = stripped[route.end():]
        for meth in METHODS:
            if re.search(r"\b" + meth + r"\(", handler):
                out.add(meth.upper() + " " + route.group(1))
for line in sorted(out):
    print(line)
PYEOF
}

# Found, not named. Naming the router files by path meant that splitting
# one of them — `crates/webapi/src/lib.rs` into `router/{mail,rest}.rs` on
# 2026-08-02 — silently emptied this side of the comparison: 48 accounted
# differences became 223, every one of them a route the script could no
# longer see rather than a route that had gone missing. A gate that reads
# a hard-coded path reports on where the code used to be.
lane() { grep -rl --include='*.rs' '\.route(' "$1" 2>/dev/null | sort; }

FC=$(extract $(lane crates/webapi/src))
MONO=$(extract $(lane crates/server/src/web))

allowed() {
    [ -f "$ALLOW" ] || return 1
    grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$ALLOW" |
        cut -d'|' -f1 |
        sed 's/[[:space:]]*$//' |
        grep -qxF "$1"
}

only_fc=$(comm -13 <(printf '%s\n' "$MONO" | sort) <(printf '%s\n' "$FC" | sort))
only_mono=$(comm -23 <(printf '%s\n' "$MONO" | sort) <(printf '%s\n' "$FC" | sort))

echo "fastcore lane (crates/webapi):      $(printf '%s\n' "$FC" | grep -c . || true) routes"
echo "monolith lane (crates/server/web):  $(printf '%s\n' "$MONO" | grep -c . || true) routes"

if [ -f "$ALLOW" ]; then
    echo
    echo "allow-list ($ALLOW) — $(grep -cv -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$ALLOW" || true) entries:"
    grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$ALLOW" | sed 's/^/    /'
fi

unaccounted=0

# Reads routes one per line: a route is "METHOD /path", which word
# splitting would tear in half.
report() {
    label="$1"
    routes="$2"
    first=1
    while IFS= read -r route; do
        [ -n "$route" ] || continue
        if allowed "$route"; then continue; fi
        if [ "$first" -eq 1 ]; then
            echo
            echo "!! only on the $label lane:"
            first=0
        fi
        echo "    $route"
        unaccounted=$((unaccounted + 1))
    done <<< "$routes"
}

report "fastcore" "$only_fc"
report "monolith" "$only_mono"

echo
if [ "$unaccounted" -eq 0 ]; then
    echo "REST parity OK — every difference is accounted for"
    exit 0
fi

echo "$unaccounted route(s) above are served by one lane and not the other,"
echo "and are not in the allow-list. One client talks to whichever lane is"
echo "deployed, so each is a feature that works or 405s depending on where"
echo "the request lands."
echo
echo "Either implement it on both lanes — putting the shared behaviour in a"
echo "stone, not a second copy of the handler — or add a line to"
echo "$ALLOW in the form:"
echo
echo "    METHOD /path    | why this lane only"
exit 1
