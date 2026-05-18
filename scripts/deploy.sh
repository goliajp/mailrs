#!/usr/bin/env bash
# usage: ./scripts/deploy.sh [--web-only]
# --web-only: skip Rust cross-compilation, only deploy web assets
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WEB_ONLY=false
if [[ "${1:-}" == "--web-only" ]]; then
  WEB_ONLY=true
fi

SSH_KEY="${SSH_KEY:-$HOME/keys/aws.pem}"
SSH_HOST="${SSH_HOST:-root@t02.golia.jp}"
REMOTE_DIR="/apps/mailrs"
TARGET="aarch64-unknown-linux-gnu"
DEPLOY_DIR="deploy"

SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no"
SCP="scp $SSH_OPTS"
SSH="ssh $SSH_OPTS $SSH_HOST"

echo "==> building web frontend"
(cd web && bunx --bun tsc -b && bunx --bun vite build)

if [ "$WEB_ONLY" = true ]; then
  echo "==> web-only mode: skipping Rust compilation"
else
  echo "==> cross-compiling for $TARGET"
  cargo zigbuild --release --target "$TARGET"

  # respect CARGO_TARGET_DIR / [build].target-dir / cargo wrappers
  TARGET_DIR="$(cargo metadata --format-version 1 --no-deps --offline 2>/dev/null \
    | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"
  BINARY="$TARGET_DIR/$TARGET/release/mailrs-server"
  if [ ! -f "$BINARY" ]; then
    echo "error: binary not found at $BINARY"
    exit 1
  fi
  echo "==> binary size: $(du -h "$BINARY" | cut -f1)"

  echo "==> uploading binary to $SSH_HOST:$REMOTE_DIR/bin/"
  $SSH "mkdir -p $REMOTE_DIR/bin $REMOTE_DIR/web"
  $SCP "$BINARY" "$SSH_HOST:$REMOTE_DIR/bin/mailrs-server"
fi

echo "==> uploading web assets"
$SSH "mkdir -p $REMOTE_DIR/web"
$SCP -r web/dist/* "$SSH_HOST:$REMOTE_DIR/web/"

if [ "$WEB_ONLY" = false ]; then
  echo "==> uploading deployment configs"
  $SCP "$DEPLOY_DIR/Dockerfile" "$SSH_HOST:$REMOTE_DIR/Dockerfile"
  $SCP "$DEPLOY_DIR/docker-compose.yml" "$SSH_HOST:$REMOTE_DIR/docker-compose.yml"
  $SCP scripts/init-schema.sql "$SSH_HOST:$REMOTE_DIR/init-schema.sql"

  # upload .env
  echo "==> uploading .env"
  $SCP "$DEPLOY_DIR/.env.production" "$SSH_HOST:$REMOTE_DIR/.env"

  # upload and run migration scripts
  echo "==> running database migrations"
  for migration in scripts/migrate-*.sql; do
    if [ -f "$migration" ]; then
      $SCP "$migration" "$SSH_HOST:$REMOTE_DIR/$(basename "$migration")"
      $SSH "cd $REMOTE_DIR && docker compose exec -T postgres psql -U mailrs -d mailrs < $(basename "$migration")" || true
    fi
  done
fi

echo "==> rebuilding and restarting container"
$SSH "cd $REMOTE_DIR && docker compose build --no-cache && docker compose up -d"

echo "==> waiting for startup..."
sleep 3

echo "==> container logs (tail 20):"
$SSH "cd $REMOTE_DIR && docker compose logs --tail 20 mailrs 2>&1" || true

echo "==> deploy complete"
