#!/usr/bin/env bash
# check-dead-routes.sh — every registered route has a caller, or a reason.
#
# The class this catches: a capability that exists on one side of the
# wire and nowhere on the other, with nothing saying so. Found by hand
# on 2026-08-10 — `POST /api/scheduled/{id}/cancel` had no caller in
# either client on the same day iOS learned to *create* a scheduled
# send, and the signature store had three live routes and no client at
# all while the web kept its signature in localStorage.
#
# A route with no client is not automatically wrong. It is wrong when
# nobody has said why, which is what the allow-file is for.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

ALLOW=scripts/dead-routes-allowed.txt

python3 - "$ALLOW" <<'PYEOF'
import glob
import re
import sys

allow_path = sys.argv[1]


def norm(p: str) -> str:
    """Collapse every path parameter to `{}` so a caller that
    interpolates an id matches the route that declares one."""
    p = re.sub(r"\{[^}]*\}", "{}", p.strip().rstrip("/"))
    # A caller writes the value, not the parameter name: an id, a
    # fixture's `thread-1`, an encoded Message-ID.
    p = re.sub(r"/(\d+|<[^>]*>|t\d+|thread[-%][^/]*)(?=/|$)", "/{}", p)
    return p


routes = set()
for path in glob.glob("crates/webapi/src/router/*.rs"):
    for m in re.finditer(r'"(/api/[^"]*)"', open(path).read()):
        routes.add(norm(m.group(1)))

# Callers. The web's `wireFetch` takes paths without the `/api`
# prefix and adds it, so both spellings count.
called = set()
web = [f for ext in ("ts", "tsx") for f in glob.glob(f"web/src/**/*.{ext}", recursive=True)]
for path in web + glob.glob("ios/Mailrs/**/*.swift", recursive=True):
    text = open(path).read()
    # Interpolations collapse *before* the scan, not after: a TS
    # `${encodeURIComponent(key)}` carries brackets that no path
    # character class contains, so a literal holding one was invisible
    # and its route looked dead. `/api/auth/external/${...}` is
    # reached from an `href`, and this is how it was being missed.
    text = re.sub(r"\$\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}", "{}", text)
    text = re.sub(r"\\\([^()]*(?:\([^()]*\)[^()]*)*\)", "{}", text)
    for m in re.finditer(r"""['"`](/(?:api/)?[a-zA-Z0-9/_{}$.:%-]*)['"`]""", text):
        raw = m.group(1)
        if not raw.startswith("/api"):
            raw = "/api" + raw
        called.add(norm(raw.split("?")[0]))
    # Paths built a segment at a time: `.../\(verb)` in Swift,
    # `${id}/star` in TS. Capture the literal suffix so a dynamically
    # assembled route still counts as called.
    for m in re.finditer(r"""/\{\}/([a-z][a-z0-9-]*)""", text):
        called.add(m.group(1))

allowed = {}
for line in open(allow_path):
    line = line.strip()
    if not line or line.startswith("#"):
        continue
    route, _, reason = line.partition("#")
    allowed[norm(route.strip())] = reason.strip()

dead = []
for route in sorted(routes):
    if route in called or route in allowed:
        continue
    # A route whose last segment is named by some client counts: that
    # is the dynamically built form above.
    if route.rsplit("/", 1)[-1] in called:
        continue
    dead.append(route)

stale = sorted(r for r in allowed if r not in routes)

if dead:
    print("!! registered, but no client calls them and no reason is given:")
    for route in dead:
        print(f"   {route}")
    print(f"\n   Either wire a client, delete the route, or add it to {allow_path}")
    print("   with a reason after a `#`.")
if stale:
    print(f"!! {allow_path} names routes that no longer exist:")
    for route in stale:
        print(f"   {route}")

if dead or stale:
    sys.exit(1)
print(f"routes: {len(routes)} registered, {len(allowed)} allowed without a client")
PYEOF
