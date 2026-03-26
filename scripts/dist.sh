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

echo "==> packaging dist"
rm -rf "$DIST"
mkdir -p "$DIST/bin" "$DIST/web"

cp "target/$TARGET/release/mailrs-server" "$DIST/bin/"
cp -r web/dist/* "$DIST/web/"
cp Dockerfile "$DIST/"
cp deploy/docker-compose.yml "$DIST/"

echo "==> done: $DIST/"
ls -lh "$DIST/bin/mailrs-server"
echo ""
find "$DIST" -maxdepth 2 -type f | sort
