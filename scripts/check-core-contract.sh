#!/usr/bin/env bash
# check-core-contract.sh — everything the core client asks for is served,
# and nothing a core serves goes unasked.
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
# `scripts/core-contract-allow.txt` names it with a reason.
#
# And the reverse, for both cores, by method and path:
#
#     routes a core registers   vs   (method, path) the client sends
#
# A route nothing asks for is either dead surface — it answered a client
# method that no longer exists, so it rots unexercised and still has to be
# kept alike in both cores — or an operator route run by hand. The second
# kind is listed in `scripts/core-routes-unasked-allow.txt` with its
# purpose; anything else fails. A listed route that the client now asks
# for, or that no core serves any more, fails too, so the list cannot
# outlive what it describes.
#
# Exit 0 = every path the client can ask for has somewhere to land, and
# every route a core serves is asked for or listed.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

ALLOW="scripts/core-contract-allow.txt"
UNASKED="scripts/core-routes-unasked-allow.txt"

python3 - "$ALLOW" "$UNASKED" <<'PYEOF'
import glob
import re
import sys

allow_path = sys.argv[1]
unasked_path = sys.argv[2]


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
    print("Each path above is a 404 the moment something calls that client")
    print("method. Either register the route in fastcore's router, or add a")
    print(f"line to {allow_path} in the form:")
    print()
    print("    /v1/path/*    | why nothing serves this")

# ── the reverse: what each core serves that nothing asks for ────────
#
# By (method, path). A path the client reads with GET and a core also
# accepts POST on is a verb nobody sends, and kept alive it is one more
# behaviour the two cores must agree on with nothing checking that they do.

VERBS = ("get", "post", "put", "delete", "patch")
HELPER = {
    "get_authed": "GET",
    "post_authed_json": "POST",
    "post_authed_no_body": "POST",
    "put_authed_json": "PUT",
    "delete_authed": "DELETE",
}


def call_body(src, open_paren):
    depth, i = 1, open_paren
    while i < len(src) and depth:
        i += 1
        if src[i:i + 1] == "(":
            depth += 1
        elif src[i:i + 1] == ")":
            depth -= 1
    return src[open_paren + 1:i]


blind = []


def routes_of(files):
    """(METHOD, path) for every `.route(` call, by constant or literal."""
    out = set()
    for f, src in files.items():
        for m in re.finditer(r"\.route\(", src):
            body = call_body(src, m.end() - 1)
            c = re.search(r'\bPATH_[A-Z0-9_]+|"(/v1[^"]*)"', body)
            if not c:
                continue
            if c.group(1):
                url = c.group(1)
            elif c.group(0) in declared:
                url = declared[c.group(0)]
            else:
                blind.append(f"{f}: {c.group(0)} is not declared in core-api")
                continue
            verbs = set(re.findall(r"\b(" + "|".join(VERBS) + r")\s*\(", body))
            if not verbs:
                blind.append(f"{f}: no HTTP verb in the route for {url}")
            for v in verbs:
                out.add((v.upper(), url))
    return out


# Each client method: the paths it builds and the verb it sends them with.
# A method whose verb cannot be read would silently ask for nothing.
sent = set()
for path, src in read("crates/core-api/src/client*.rs").items():
    for body in re.split(r"\n    (?:pub(?:\([a-z]+\))? )?async fn ", src)[1:]:
        urls = re.findall(r'"(/v1[^"]*)"', body)
        if not urls:
            continue
        verbs = {HELPER[h] for h in re.findall(r"\b(" + "|".join(HELPER) + r")\b", body)}
        verbs |= {v.upper() for v in re.findall(r"\.(get|post|put|delete|patch)\(self\.url", body)}
        if not verbs:
            blind.append(f"{path}: no verb for {urls[0]}")
        for u in urls:
            for v in verbs:
                sent.add((v, norm(u)))


def asked_for(route):
    verb, url = route
    return any(v == verb and matches(p, url) for v, p in sent)


listed = {}
for line in open(unasked_path):
    line = line.strip()
    if not line or line.startswith("#"):
        continue
    key, _, why = line.partition("|")
    parts = key.split()
    if len(parts) != 2 or not why.strip():
        blind.append(f"{unasked_path}: not 'METHOD /path | purpose': {line}")
        continue
    listed[(parts[0], parts[1])] = why.strip()

cores = {
    "fastcore": read("crates/fastcore/src/**/*.rs"),
    "pg-core": {f: s for f, s in read("crates/server/src/core_rpc/**/*.rs").items()
                if "/tests/" not in f},
}
served_by = {name: routes_of(files) for name, files in cores.items()}

dead = []
for name, routes in served_by.items():
    unasked = sorted(r for r in routes if not asked_for(r))
    kept = [r for r in unasked if r in listed]
    print(f"{name:9}: {len(routes)} routes, {len(unasked)} not asked for, {len(kept)} of them listed")
    dead += [f"{name}: {v} {u}" for v, u in unasked if (v, u) not in listed]

every = set().union(*served_by.values())
stale = sorted(f"{v} {u}" for (v, u) in listed if (v, u) not in every or asked_for((v, u)))

if blind:
    print()
    print("!! the extraction cannot see part of the contract:")
    for b in blind:
        print(f"    {b}")
if dead:
    print()
    print("!! a core serves these and no client method asks for them:")
    for d in dead:
        print(f"    {d}")
    print()
    print("Delete the route, its handler and the core-api types only it used,")
    print(f"from both cores; or, for an operator route run by hand, list it in")
    print(f"{unasked_path} as 'METHOD /path | what it is for'.")
if stale:
    print()
    print(f"!! listed in {unasked_path} but served by no core, or now asked for:")
    for s in stale:
        print(f"    {s}")

print()
if not unaccounted and not unresolved and not blind and not dead and not stale:
    print("core contract OK — every path the client asks for is served, and")
    print("every route a core serves is asked for or listed")
    sys.exit(0)
sys.exit(1)
PYEOF
