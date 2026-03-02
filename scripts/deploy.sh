#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SSH_KEY="${SSH_KEY:-$HOME/keys/aws.pem}"
SSH_HOST="${SSH_HOST:-root@t02.golia.jp}"
REMOTE_DIR="/apps/mailrs"
TARGET="aarch64-unknown-linux-gnu"

SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no"
SCP="scp $SSH_OPTS"
SSH="ssh $SSH_OPTS $SSH_HOST"

echo "==> building web frontend"
(cd web && bun run build)

echo "==> cross-compiling for $TARGET"
cargo zigbuild --release --target "$TARGET"

BINARY="target/$TARGET/release/mailrs-server"
echo "==> binary size: $(du -h "$BINARY" | cut -f1)"

echo "==> uploading binary to $SSH_HOST:$REMOTE_DIR/bin/"
$SSH "mkdir -p $REMOTE_DIR/bin $REMOTE_DIR/web"
$SCP "$BINARY" "$SSH_HOST:$REMOTE_DIR/bin/mailrs-server"

echo "==> uploading web assets"
$SCP -r web/dist/* "$SSH_HOST:$REMOTE_DIR/web/"

echo "==> rebuilding and restarting container"
$SSH "cd $REMOTE_DIR && docker compose build --no-cache && docker compose up -d"

echo "==> container logs (tail 10):"
$SSH "cd $REMOTE_DIR && docker logs --tail 10 mailrs 2>&1" || true

echo "==> deploy complete"
