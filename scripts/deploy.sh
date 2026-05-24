#!/usr/bin/env bash
# usage: ./scripts/deploy.sh [--web-only]
# --web-only: skip Rust cross-compilation, only deploy web assets
#
# Health gate flow (added v0.7):
# 1. pre-deploy: confirm old container is healthy (so we have a known-
#    good baseline; refuse to deploy on top of an already-broken prod)
# 2. backup old binary to ~/backup/ before upload
# 3. deploy (build / upload / restart)
# 4. post-deploy: poll new container's /api/health until 200 or 60s
# 5. if post-deploy fails: rollback (restore backup binary + restart)
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
BACKUP_DIR="/root/backup"
HEALTH_URL="${HEALTH_URL:-http://localhost:3200/api/health}"
HEALTH_TIMEOUT_SECS=60

SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no"
SCP="scp $SSH_OPTS"
SSH="ssh $SSH_OPTS $SSH_HOST"

# ---------- helpers ----------

remote_curl_health() {
  # GET /api/health, parse 'status' field. Returns 0 + prints status
  # on success; non-zero if curl or parse fails.
  $SSH "curl -sS --max-time 5 $HEALTH_URL 2>/dev/null \
    | python3 -c \"import json,sys; d=json.load(sys.stdin); print(d.get('status','?'))\""
}

wait_for_healthy() {
  # Poll /api/health every 2s up to HEALTH_TIMEOUT_SECS. Prints status
  # on success. Returns 0 if 'healthy' or 'degraded'; non-zero if
  # 'unhealthy' or timed out.
  local elapsed=0
  while [ "$elapsed" -lt "$HEALTH_TIMEOUT_SECS" ]; do
    status=$(remote_curl_health 2>/dev/null || echo "unreachable")
    case "$status" in
      healthy|degraded)
        echo "$status"
        return 0
        ;;
      *)
        sleep 2
        elapsed=$((elapsed + 2))
        ;;
    esac
  done
  echo "TIMEOUT after ${HEALTH_TIMEOUT_SECS}s; last status: $status"
  return 1
}

# ---------- pre-deploy health baseline + backup ----------

if [ "$WEB_ONLY" = false ]; then
  echo "==> pre-deploy health check (current prod)"
  if pre_status=$(remote_curl_health 2>/dev/null); then
    echo "    current status: $pre_status"
    case "$pre_status" in
      unhealthy)
        echo "error: prod is already unhealthy — refusing to deploy on top." >&2
        echo "investigate first, or set FORCE_DEPLOY=1 to override." >&2
        if [ "${FORCE_DEPLOY:-0}" != "1" ]; then
          exit 1
        fi
        echo "    FORCE_DEPLOY=1 — proceeding anyway"
        ;;
    esac
  else
    echo "    warning: could not reach $HEALTH_URL — assuming first deploy / new host"
  fi

  echo "==> backing up current binary"
  TS=$(date +%Y%m%d-%H%M%S)
  $SSH "mkdir -p $BACKUP_DIR && \
    if [ -f $REMOTE_DIR/bin/mailrs-server ]; then \
      cp $REMOTE_DIR/bin/mailrs-server $BACKUP_DIR/mailrs-server.$TS && \
      echo '    backup: $BACKUP_DIR/mailrs-server.$TS ('\$(du -h $BACKUP_DIR/mailrs-server.$TS | cut -f1)')'; \
    else \
      echo '    skip backup: no prior binary at $REMOTE_DIR/bin/mailrs-server'; \
    fi"
  echo "$TS" > /tmp/mailrs-deploy-ts
fi

# ---------- build ----------

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

# ---------- post-deploy health verify + rollback ----------

if [ "$WEB_ONLY" = false ]; then
  echo "==> waiting for new container to become healthy (timeout ${HEALTH_TIMEOUT_SECS}s)"
  if post_status=$(wait_for_healthy); then
    echo "==> health: $post_status — deploy ok"
  else
    echo "error: post-deploy health check failed: $post_status" >&2
    echo "==> attempting rollback"

    TS=$(cat /tmp/mailrs-deploy-ts 2>/dev/null || echo "")
    if [ -n "$TS" ] && $SSH "[ -f $BACKUP_DIR/mailrs-server.$TS ]"; then
      echo "    restoring binary from $BACKUP_DIR/mailrs-server.$TS"
      $SSH "cp $BACKUP_DIR/mailrs-server.$TS $REMOTE_DIR/bin/mailrs-server && \
        cd $REMOTE_DIR && docker compose up -d --force-recreate mailrs"

      if rb_status=$(wait_for_healthy); then
        echo "    rollback ok — restored to baseline ($rb_status)"
      else
        echo "    ROLLBACK FAILED ($rb_status) — manual intervention required" >&2
      fi
    else
      echo "    no backup available — cannot rollback automatically" >&2
    fi

    echo "==> container logs (tail 40):"
    $SSH "cd $REMOTE_DIR && docker compose logs --tail 40 mailrs 2>&1" || true
    exit 1
  fi
fi

echo "==> container logs (tail 20):"
$SSH "cd $REMOTE_DIR && docker compose logs --tail 20 mailrs 2>&1" || true

echo "==> deploy complete"
