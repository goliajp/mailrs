#!/usr/bin/env bash
# check-core-contract.sh — everything the core client asks for is served.
#
# Replaces check-rest-parity.sh, which compared `crates/webapi` against
# `crates/server/src/web`. The monolith is not coming back, so that
# comparison measured production against code that will never run: every
# new production route failed it and needed an allow-list line, and the
# list had reached 51 entries of paperwork.
#
# What is actually load-bearing is the core RPC contract, and it is held
# together by nothing at all today. `crates/core-api` declares 179
# `PATH_*` constants; fastcore's router registers routes *by constant*,
# but the client does not use them — every `CoreApiClient` method spells
# its path again in a `format!`. A typo on either side is a 404 in
# production and nothing catches it.
#
# So this compares the two sides that both run:
#
#     paths the core client constructs   vs   routes fastcore registers
#
# A path asked for and not served fails, unless
# `.claude/core-contract-allow.txt` names it with a reason.
#
# Exit 0 = every path the client can ask for has somewhere to land.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

ALLOW=".claude/core-contract-allow.txt"

python3 - "$ALLOW" <<'PYEOF'
import glob
import re
import sys

allow_path = sys.argv[1]


def read(pattern):
    return {f: open(f).read() for f in glob.glob(pattern, recursive=True)}


# ── the constants ───────────────────────────────────────────────────
#
# `=\s*"`, not `= "`. rustfmt breaks the line after `=` when the path is
# long, and a regex that wanted a space there silently dropped every
# two-line declaration — including the one this gate was first pointed
# at. The measuring device's failure looked exactly like a finding: four
# paths reported unserved, three of them because their constant had not
# been read.
DECL = re.compile(r'pub const (PATH_[A-Z0-9_]+): &str =\s*"([^"]+)"')
declared = {}
for src in read("crates/core-api/src/**/*.rs").values():
    for m in DECL.finditer(src):
        declared[m.group(1)] = m.group(2)

# ── what fastcore serves ────────────────────────────────────────────
#
# By constant or by literal; a `//` comment may sit between `.route(`
# and its first argument.
ROUTE = re.compile(r'\.route\(\s*(?://[^\n]*\n\s*)*(?:[a-z_]+::)?(PATH_[A-Z0-9_]+|"[^"]+")')
served, unresolved = set(), set()
for src in read("crates/fastcore/src/**/*.rs").values():
    for m in ROUTE.finditer(src):
        token = m.group(1)
        if token.startswith('"'):
            served.add(token.strip('"'))
        elif token in declared:
            served.add(declared[token])
        else:
            # A constant the router names and core-api does not declare.
            # Reported rather than skipped: it means one of the two
            # extractions has gone blind, which is the failure this gate
            # cannot afford.
            unresolved.add(token)

# ── what the client asks for ────────────────────────────────────────
called = {}
for path, src in read("crates/core-api/src/client*.rs").items():
    for m in re.finditer(r'format!\(\s*"(/v1[^"]*)"', src):
        called.setdefault(m.group(1), path)
    for m in re.finditer(r'"(/v1[^"{}]*)"', src):
        called.setdefault(m.group(1), path)


def norm(p):
    """Placeholders and query strings do not distinguish a route."""
    return re.sub(r"\{[^}]*\}", "*", p.split("?")[0]).rstrip("/")


# Matched segment by segment rather than as whole strings.
#
# The first attempt compared normalised strings and skipped anything
# ending in `*` as "built from a variable" — which dropped 17 of 54
# paths, a third of the surface, including several that are served.
# `/v1/admin/accounts/{address}` ends in a placeholder like any REST
# path does; only `thread_action` really chooses its last segment at
# runtime, and no rule on shape alone tells those two apart.
#
# So: a `*` on the client side matches whatever the route has in that
# position, and a **literal must match that literal**. Letting a route
# placeholder absorb a client literal as well seemed symmetrical and
# made the gate match everything — injecting `by-messge-id` into the
# client and unregistering the route it needs both passed, because some
# other six-segment route always had a placeholder in the right place.
#
# One-directional, it lets `/v1/users/*/threads/*/*` find
# `…/threads/{thread_id}/read` — the route `thread_action("read")` asks
# for — while `/v1/mailboxes/*/messages/uid/*/raw` matches nothing.
def matches(asked_path, route):
    a, b = asked_path.strip("/").split("/"), norm(route).strip("/").split("/")
    if len(a) != len(b):
        return False
    return all(x == "*" or x == y for x, y in zip(a, b))


asked = {norm(p): src for p, src in called.items()}
skipped = []
missing = sorted(p for p in asked if not any(matches(p, r) for r in served))

allowed = set()
try:
    for line in open(allow_path):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        allowed.add(line.split("|")[0].strip())
except OSError:
    pass

print(f"core-api declares      : {len(declared)} PATH_* constants")
print(f"fastcore serves        : {len(served)} routes")
print(f"the core client asks   : {len(asked)} paths")
if skipped:
    print(f"not structurally checkable: {len(skipped)} (path built from a variable segment)")
    for p in skipped:
        print(f"    {p}    ← {asked[p]}")

if allowed:
    print()
    print(f"allow-list ({allow_path}) — {len(allowed)} entries:")
    for line in open(allow_path):
        if line.strip() and not line.startswith("#"):
            print("    " + line.rstrip())

if unresolved:
    print()
    print("!! fastcore names a constant core-api does not declare:")
    for token in sorted(unresolved):
        print(f"    {token}")

unaccounted = [p for p in missing if p not in allowed]
if unaccounted:
    print()
    print("!! the core client asks for these and fastcore serves none of them:")
    for p in unaccounted:
        print(f"    {p}    ← {asked[p]}")

print()
if not unaccounted and not unresolved:
    print("core contract OK — every path the client asks for is served")
    sys.exit(0)

print("Each path above is a 404 the moment something calls that client")
print("method. Either register the route in fastcore's router, or add a")
print(f"line to {allow_path} in the form:")
print()
print("    /v1/path/*    | why nothing serves this")
sys.exit(1)
PYEOF
