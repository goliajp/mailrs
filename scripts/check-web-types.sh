#!/usr/bin/env bash
# The web client's API types are generated from openapi.json. A change to
# the spec without regenerating them leaves the committed file behind, and
# nothing else notices: the web's own check regenerates it in place and
# passes. Regenerate into a scratch file and compare.
set -euo pipefail
cd "$(dirname "$0")/../web"
[ -d node_modules ] || bun install --frozen-lockfile >/dev/null
tmp=$(mktemp -t mailrs-types.XXXXXX)
trap 'rm -f "$tmp"' EXIT
bunx openapi-typescript public/openapi.json -o "$tmp" >/dev/null 2>&1
if ! cmp -s "$tmp" src/lib/types.openapi.ts; then
    echo "web/src/lib/types.openapi.ts is behind public/openapi.json: run 'bun run gen-types' in web/ and commit it" >&2
    exit 1
fi
