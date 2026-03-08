#!/usr/bin/env bash
# usage: ./scripts/bump.sh <new-version>
# bumps workspace version in Cargo.toml and web/package.json
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <version>  (e.g. 0.5.0)"
  exit 1
fi

VERSION="$1"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# validate semver-ish format
if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+'; then
  echo "error: version must be semver (e.g. 1.2.3)"
  exit 1
fi

echo "bumping to v${VERSION}"

# 1. workspace Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"${VERSION}\"/" "$ROOT/Cargo.toml"

# 2. web/package.json
sed -i '' "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" "$ROOT/web/package.json"

echo "done. files updated:"
grep -n "version" "$ROOT/Cargo.toml" | head -1
grep -n "version" "$ROOT/web/package.json" | head -1
