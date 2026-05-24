#!/usr/bin/env python3
"""Generate stub entries in openapi.json for every endpoint in the
REST router that's currently missing.

Stubs include only:
- method(s) inferred from `.route("...", get(...).post(...)...)`-style code
- summary derived from path (auto-generated)
- one `responses["200"]` placeholder

Detailed parameters / request bodies / response schemas remain TODO —
this script eliminates the existence-drift; quality drift is a
follow-up (v0.5b).

Run from repo root:
    python3 scripts/openapi-stub-missing.py
"""
import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "crates" / "server" / "src"
OPENAPI = ROOT / "web" / "public" / "openapi.json"

# Map .route() chained methods → openapi method names
METHOD_FNS = {"get", "post", "put", "delete", "patch", "head", "options", "any"}


def parse_router_endpoints():
    """Walk all .rs files, find `.route("path", get(...).post(...))` etc.
    Return dict: path -> set(of method names).
    """
    endpoints = defaultdict(set)
    # Pattern: .route( <path> , <method chain>)
    # Use single regex that captures path + everything to the closing paren.
    route_re = re.compile(
        r'\.route\s*\(\s*"([^"]+)"\s*,\s*((?:[^)(]*(?:\([^)]*\))?)*)\)',
        re.DOTALL,
    )
    for rs in SRC.rglob("*.rs"):
        text = rs.read_text()
        for m in route_re.finditer(text):
            path = m.group(1)
            chain = m.group(2)
            for fn in METHOD_FNS:
                if re.search(rf'\b{fn}\s*\(', chain):
                    if fn == "any":
                        # `any()` matches any verb — list common ones
                        for v in ["get", "post", "put", "delete", "patch"]:
                            endpoints[path].add(v)
                    else:
                        endpoints[path].add(fn)
    return endpoints


def path_to_summary(path: str, method: str) -> str:
    """Turn `/api/admin/oauth-clients/{client_id}` into a human summary."""
    verb = {
        "get": "Get",
        "post": "Create",
        "put": "Update",
        "delete": "Delete",
        "patch": "Patch",
        "head": "Head",
        "options": "Options",
    }.get(method, method.title())
    # strip /api/, replace -/_ with spaces, drop {param} segments
    clean = re.sub(r"\{[^}]+\}", "", path)
    clean = clean.strip("/").replace("api/", "")
    words = re.split(r"[-_/]+", clean)
    name = " ".join(w for w in words if w)
    return f"{verb} {name}" if name else f"{verb} endpoint"


def stub_operation(path: str, method: str) -> dict:
    return {
        "summary": path_to_summary(path, method),
        "description": "Stub — full schema TBD. See REFACTOR-V2-v0.5-api-drift.md.",
        "responses": {
            "200": {
                "description": "Success",
                "content": {
                    "application/json": {
                        "schema": {"type": "object"}
                    }
                },
            }
        },
    }


def main():
    if not OPENAPI.exists():
        print(f"ERROR: {OPENAPI} not found", file=sys.stderr)
        sys.exit(1)

    spec = json.loads(OPENAPI.read_text())
    existing = set(spec.get("paths", {}).keys())
    router = parse_router_endpoints()
    missing = sorted(set(router.keys()) - existing)

    if not missing:
        print("OK: no missing endpoints")
        return

    print(f"Adding {len(missing)} stubs to openapi.json:")
    for path in missing:
        methods = sorted(router[path])
        spec["paths"].setdefault(path, {})
        for m in methods:
            spec["paths"][path][m] = stub_operation(path, m)
        print(f"  + {path}  ({', '.join(methods)})")

    # Pretty-write back with 2-space indent (matches existing style)
    OPENAPI.write_text(json.dumps(spec, indent=2) + "\n")
    print(f"\nWrote {OPENAPI} with {len(spec['paths'])} total endpoints")


if __name__ == "__main__":
    main()
