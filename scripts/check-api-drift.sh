#!/usr/bin/env bash
# Forbid API drift between REST router and openapi.json.
# Pre-flight: run before release.sh; fails non-zero if any router
# endpoint is missing from openapi.json (or vice versa).
#
# Adds 0 lines per scrape — fast enough to gate every commit.
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
import json, os, re, sys
routes = set()
for root, _, files in os.walk('crates/server/src'):
    for f in files:
        if not f.endswith('.rs'):
            continue
        text = open(os.path.join(root, f)).read()
        for m in re.finditer(r'\.route\s*\(\s*"([^"]+)"', text):
            routes.add(m.group(1))

api = set(json.load(open('web/public/openapi.json'))['paths'].keys())

missing_in_api = sorted(routes - api)
extra_in_api = sorted(api - routes)

if missing_in_api or extra_in_api:
    if missing_in_api:
        print('FAIL: router has endpoints missing from openapi.json:', file=sys.stderr)
        for p in missing_in_api:
            print(f'  + {p}  (add to openapi.json — see scripts/openapi-stub-missing.py)',
                  file=sys.stderr)
    if extra_in_api:
        print('FAIL: openapi.json has phantom endpoints:', file=sys.stderr)
        for p in extra_in_api:
            print(f'  - {p}  (remove from openapi.json or restore route)',
                  file=sys.stderr)
    sys.exit(1)

print(f'OK: router and openapi.json in sync ({len(routes)} endpoints)')
PY
