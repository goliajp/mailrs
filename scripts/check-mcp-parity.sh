#!/usr/bin/env bash
# check-mcp-parity.sh — assert both MCP lanes expose the same tool set.
#
# mailrs ships two independent MCP implementations (project rule
# `feedback-fastcore-core-mode-parity`): the fastcore lane
# (kevy-backed, what prod runs) and the monolith lane (spg SQL, the
# staging dogfood lane). Neither declares `name = "..."` on its
# `#[tool]` attributes, so **the exposed MCP tool name is the Rust
# function name** — a rename on one side silently breaks every agent
# that talks to the other.
#
# That is exactly how the two lanes drifted to 30 unique names each
# by 2026-07-18. This script is the regression gate: run it after any
# change under either mcp/ directory.
#
# Exit 0 = the two tool sets are identical.
set -euo pipefail
cd "$(dirname "$0")/.."

extract() {
    # Print the function name following each #[tool(...)] attribute.
    # The attribute may span multiple lines, so scan forward for the
    # next `fn <name>(`.
    python3 - "$@" <<'PY'
import re, sys
names = []
for path in sys.argv[1:]:
    with open(path) as fh:
        src = fh.read()
    # Only real attributes: `#[tool(` at the start of a line (modulo
    # indentation). Matching anywhere would also catch doc comments
    # that mention the macro, which is how auth.rs's middleware first
    # showed up as a phantom tool.
    for m in re.finditer(r'^[ \t]*#\[tool\(', src, re.M):
        fn = re.search(r'\n\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z_0-9]+)',
                       src[m.start():])
        if fn:
            names.append(fn.group(1))
print('\n'.join(sorted(set(names))))
PY
}

# Found, not named — for the reason recorded in check-rest-parity.sh: a
# `*.rs` glob stops at the directory it names, so the first tool moved into
# a subdirectory would leave this comparing against a lane it can no longer
# fully see, and report parity.
lane() { grep -rl --include='*.rs' '#\[tool' "$1" 2>/dev/null | sort; }

MONO=$(extract $(lane crates/server/src/mcp))
FC=$(extract $(lane crates/webapi/src/handlers/mcp))

only_mono=$(comm -23 <(printf '%s\n' "$MONO") <(printf '%s\n' "$FC"))
only_fc=$(comm -13 <(printf '%s\n' "$MONO") <(printf '%s\n' "$FC"))

n_mono=$(printf '%s\n' "$MONO" | grep -c . || true)
n_fc=$(printf '%s\n' "$FC" | grep -c . || true)
echo "monolith lane: $n_mono tools"
echo "fastcore lane: $n_fc tools"

if [ -z "$only_mono" ] && [ -z "$only_fc" ]; then
    echo "MCP parity OK — both lanes expose the same $n_fc tools"
    exit 0
fi

echo
echo "!! MCP PARITY BROKEN"
if [ -n "$only_mono" ]; then
    echo "  only in monolith ($(printf '%s\n' "$only_mono" | grep -c .)):"
    printf '    %s\n' $only_mono
fi
if [ -n "$only_fc" ]; then
    echo "  only in fastcore ($(printf '%s\n' "$only_fc" | grep -c .)):"
    printf '    %s\n' $only_fc
fi
echo
echo "Every tool must exist on BOTH lanes with the same name — the name"
echo "is the wire contract. Add the missing implementation rather than"
echo "renaming one side away."
exit 1
