#!/usr/bin/env bash
# Forbid drift between the production REST router and openapi.json.
#
# openapi.json documents what production answers, and production is
# crates/webapi. Every (path, method) the router registers must be in the
# spec, and every one the spec lists must be registered. Exits non-zero on
# any difference.
#
# A route registered with `any(...)` serves every verb (DAV collections
# use it for PROPFIND / REPORT), so for those only the path is compared.
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
import glob, json, re, sys

FILES = ['crates/webapi/src/router.rs'] + sorted(glob.glob('crates/webapi/src/router/*.rs'))
VERBS = ('get', 'post', 'put', 'delete', 'patch')
# the catch-all that turns an unknown /api path into a 404
IGNORED = {'/api/{rest}'}


def strip_comments(text):
    out = []
    for line in text.split('\n'):
        # a route string never contains `//` followed by a space
        i = line.find('// ')
        if i < 0 and line.rstrip().endswith('//'):
            i = line.rfind('//')
        out.append(line if i < 0 else line[:i])
    return '\n'.join(out)


routes = {}
for path in FILES:
    text = strip_comments(open(path).read())
    # rustfmt may break `.route(` and its path string onto separate lines
    for m in re.finditer(r'\.route\s*\(\s*"([^"]+)"\s*,', text):
        i, depth = m.end(), 1
        while depth:
            if i >= len(text):
                sys.exit(f'FAIL: unbalanced .route( for {m.group(1)} in {path}')
            depth += (text[i] == '(') - (text[i] == ')')
            i += 1
        body = text[m.end():i - 1]
        verbs = set(re.findall(r'\b(get|post|put|delete|patch|any)\s*\(', body))
        if not verbs:
            sys.exit(f'FAIL: no method found for route {m.group(1)} in {path}')
        # OpenAPI has no catch-all segment; `{*path}` is documented as `{path}`
        route = re.sub(r'\{\*([^}]+)\}', r'{\1}', m.group(1))
        routes.setdefault(route, set()).update(verbs)

for p in IGNORED:
    routes.pop(p, None)

if len(routes) < 100:
    sys.exit(f'FAIL: parsed only {len(routes)} routes from crates/webapi — parser broken?')

spec = json.load(open('web/public/openapi.json'))['paths']

errors = []
for p in sorted(set(routes) - set(spec)):
    errors.append(f'  + {p} {sorted(routes[p])}  (router only: add to openapi.json)')
for p in sorted(set(spec) - set(routes)):
    errors.append(f'  - {p}  (openapi.json only: remove it or restore the route)')
for p in sorted(set(routes) & set(spec)):
    if 'any' in routes[p]:
        continue
    documented = {k for k in spec[p] if k in VERBS}
    for v in sorted(routes[p] - documented):
        errors.append(f'  + {v.upper()} {p}  (router only: add to openapi.json)')
    for v in sorted(documented - routes[p]):
        errors.append(f'  - {v.upper()} {p}  (openapi.json only)')

if errors:
    print('FAIL: crates/webapi router and web/public/openapi.json differ:', file=sys.stderr)
    print('\n'.join(errors), file=sys.stderr)
    sys.exit(1)

print(f'OK: crates/webapi router and openapi.json in sync ({len(routes)} paths)')
PY
