#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SSH_KEY="${SSH_KEY:-$HOME/keys/aws.pem}"
SSH_HOST="${SSH_HOST:-root@t02.golia.jp}"
REMOTE_DIR="/apps/mailrs"
TARGET="aarch64-unknown-linux-gnu"
DIST_DIR="dist/$TARGET"

SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no"
SCP="scp $SSH_OPTS"
SSH="ssh $SSH_OPTS $SSH_HOST"

echo "==> building web frontend"
(cd web && bunx --bun tsc -b && bunx --bun vite build)

echo "==> cross-compiling for $TARGET"
cargo zigbuild --release --target "$TARGET"

BINARY="target/$TARGET/release/mailrs-server"
echo "==> binary size: $(du -h "$BINARY" | cut -f1)"

echo "==> uploading binary to $SSH_HOST:$REMOTE_DIR/bin/"
$SSH "mkdir -p $REMOTE_DIR/bin $REMOTE_DIR/web"
$SCP "$BINARY" "$SSH_HOST:$REMOTE_DIR/bin/mailrs-server"

echo "==> uploading web assets"
$SCP -r web/dist/* "$SSH_HOST:$REMOTE_DIR/web/"

echo "==> uploading deployment configs"
$SCP "$DIST_DIR/Dockerfile" "$SSH_HOST:$REMOTE_DIR/Dockerfile"
$SCP "$DIST_DIR/docker-compose.yml" "$SSH_HOST:$REMOTE_DIR/docker-compose.yml"
$SCP scripts/init-schema.sql "$SSH_HOST:$REMOTE_DIR/init-schema.sql"

# upload .env with secrets if present
if [ -f "$ROOT/.env.local" ]; then
  echo "==> uploading .env secrets"
  # convert .env.local to docker-compose compatible .env with MAILRS_ prefix
  {
    grep '^GEMINI_API_KEY=' "$ROOT/.env.local" | sed 's/^GEMINI_API_KEY=/MAILRS_GEMINI_API_KEY=/'
  } > /tmp/mailrs-deploy-env
  $SCP /tmp/mailrs-deploy-env "$SSH_HOST:$REMOTE_DIR/.env"
  rm -f /tmp/mailrs-deploy-env
fi

# upload and run migration scripts
echo "==> running database migrations"
for migration in scripts/migrate-*.sql; do
  if [ -f "$migration" ]; then
    $SCP "$migration" "$SSH_HOST:$REMOTE_DIR/$(basename "$migration")"
    $SSH "cd $REMOTE_DIR && docker compose exec -T postgres psql -U mailrs -d mailrs < $(basename "$migration")" || true
  fi
done

echo "==> rebuilding and restarting container"
$SSH "cd $REMOTE_DIR && docker compose build --no-cache && docker compose up -d"

echo "==> waiting for startup..."
sleep 3

echo "==> container logs (tail 20):"
$SSH "cd $REMOTE_DIR && docker compose logs --tail 20 mailrs 2>&1" || true

echo "==> deploy complete"
