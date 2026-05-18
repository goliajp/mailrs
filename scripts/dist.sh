#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET="${1:-aarch64-unknown-linux-gnu}"
DIST="$ROOT/dist/$TARGET"

echo "==> building web frontend"
(cd web && bun run build)

echo "==> building mailrs-server for $TARGET"
cargo zigbuild --release --target "$TARGET"

# respect CARGO_TARGET_DIR / [build].target-dir / cargo wrappers
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps --offline 2>/dev/null \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"

echo "==> packaging dist"
rm -rf "$DIST"
mkdir -p "$DIST/bin" "$DIST/web"

cp "$TARGET_DIR/$TARGET/release/mailrs-server" "$DIST/bin/"
cp -r web/dist/* "$DIST/web/"
cp Dockerfile "$DIST/"
cp deploy/docker-compose.yml "$DIST/"

echo "==> done: $DIST/"
ls -lh "$DIST/bin/mailrs-server"
echo ""
find "$DIST" -maxdepth 2 -type f | sort
