#!/usr/bin/env bash
# usage: ./scripts/deploy.sh [--web-only|--ghcr]
# --web-only: skip Rust cross-compilation, only deploy web assets
# --ghcr:     skip local cargo build entirely; deploy ghcr.io/goliajp/mailrs:<version>
#             from the CI-published multi-arch image. Version comes from Cargo.toml.
#             Uses docker-compose.prod.yml (ghcr-pull mode) instead of
#             deploy/docker-compose.yml (build-from-source).
#
# Health gate flow (added v0.7):
# 1. pre-deploy: confirm old container is healthy (so we have a known-
#    good baseline; refuse to deploy on top of an already-broken prod)
# 2. backup old binary to ~/backup/ before upload (legacy mode only)
# 3. deploy (build / upload / restart)
# 4. post-deploy: poll new container's /api/health until 200 or 60s
# 5. if post-deploy fails: rollback (legacy: restore backup binary; ghcr:
#    alert user — image-based rollback requires manual tag switch)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WEB_ONLY=false
GHCR_MODE=false
case "${1:-}" in
  --web-only) WEB_ONLY=true ;;
  --ghcr)     GHCR_MODE=true ;;
esac

if [ "$GHCR_MODE" = true ] && [ "$WEB_ONLY" = true ]; then
  echo "error: --ghcr and --web-only are mutually exclusive" >&2
  exit 1
fi

if [ "$GHCR_MODE" = true ]; then
  VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
  echo "==> --ghcr mode: targeting ghcr.io/goliajp/mailrs:$VERSION"
fi

SSH_KEY="${SSH_KEY:-$HOME/keys/aws.pem}"
SSH_HOST="${SSH_HOST:-root@t02.golia.jp}"
REMOTE_DIR="/apps/mailrs"
TARGET="aarch64-unknown-linux-gnu"
DEPLOY_DIR="deploy"
BACKUP_DIR="/root/backup"
HEALTH_URL="${HEALTH_URL:-http://localhost:3100/api/health}"
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

  if [ "$GHCR_MODE" = false ]; then
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
  else
    echo "==> ghcr mode: skip binary backup (rollback is image-tag based)"
  fi
fi

# ---------- build ----------

if [ "$GHCR_MODE" = false ]; then
  echo "==> building web frontend"
  (cd web && bunx --bun tsc -b && bunx --bun vite build)
else
  echo "==> ghcr mode: skip web build (assets ship inside ghcr image)"
fi

if [ "$WEB_ONLY" = true ]; then
  echo "==> web-only mode: skipping Rust compilation"
elif [ "$GHCR_MODE" = true ]; then
  echo "==> ghcr mode: skipping cargo zigbuild (image pulled from registry)"
else
  echo "==> cross-compiling for $TARGET"
  cargo zigbuild --release --target "$TARGET" --features render-preview

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

if [ "$GHCR_MODE" = false ]; then
  echo "==> uploading web assets"
  $SSH "mkdir -p $REMOTE_DIR/web"
  $SCP -r web/dist/* "$SSH_HOST:$REMOTE_DIR/web/"
fi

if [ "$WEB_ONLY" = false ]; then
  echo "==> uploading deployment configs"
  if [ "$GHCR_MODE" = true ]; then
    # ghcr mode: prod uses docker-compose.prod.yml (image: ghcr.io/...)
    # instead of build-from-source compose. Dockerfile not needed.
    $SCP docker-compose.prod.yml "$SSH_HOST:$REMOTE_DIR/docker-compose.yml"
  else
    $SCP "$DEPLOY_DIR/Dockerfile" "$SSH_HOST:$REMOTE_DIR/Dockerfile"
    $SCP "$DEPLOY_DIR/docker-compose.yml" "$SSH_HOST:$REMOTE_DIR/docker-compose.yml"
  fi
  $SCP scripts/init-schema.sql "$SSH_HOST:$REMOTE_DIR/init-schema.sql"
  $SCP scripts/pg-extensions.sql "$SSH_HOST:$REMOTE_DIR/pg-extensions.sql"

  # upload .env
  echo "==> uploading .env"
  $SCP "$DEPLOY_DIR/.env.production" "$SSH_HOST:$REMOTE_DIR/.env"

  if [ "$GHCR_MODE" = true ]; then
    echo "==> pinning MAILRS_VERSION=$VERSION in remote .env"
    $SSH "if grep -q '^MAILRS_VERSION=' $REMOTE_DIR/.env; then \
      sed -i 's|^MAILRS_VERSION=.*|MAILRS_VERSION=$VERSION|' $REMOTE_DIR/.env; \
    else \
      echo 'MAILRS_VERSION=$VERSION' >> $REMOTE_DIR/.env; \
    fi"
  fi

  # upload and run migration scripts
  echo "==> running database migrations"
  for migration in scripts/migrate-*.sql; do
    if [ -f "$migration" ]; then
      $SCP "$migration" "$SSH_HOST:$REMOTE_DIR/$(basename "$migration")"
      $SSH "cd $REMOTE_DIR && docker compose exec -T postgres psql -U mailrs -d mailrs < $(basename "$migration")" || true
    fi
  done
fi

if [ "$GHCR_MODE" = true ]; then
  # Align host-bind-mounted secrets + the named data volume to the image's
  # mailrs UID 10001 BEFORE starting the new container. The chown is
  # atomic (Linux metadata flip, file descriptors stay valid), so even if
  # the current mailrs container is mid-write the running process keeps
  # access. We stop mailrs first anyway to avoid any new writes landing
  # as root-owned files between the chown and the new container start.
  echo "==> aligning prod file ownership to image UID 10001 (cert + /data volume)"
  $SSH "cd $REMOTE_DIR && docker compose stop mailrs 2>/dev/null || true"
  $SSH "chown -R 10001:10001 $REMOTE_DIR/certs/ 2>/dev/null || true"
  $SSH "docker run --rm -v mailrs_mailrs-data:/data alpine chown -R 10001:10001 /data" || \
    { echo "error: failed to chown /data volume" >&2; exit 1; }

  echo "==> pulling ghcr.io/goliajp/mailrs:$VERSION and restarting"
  $SSH "cd $REMOTE_DIR && docker pull ghcr.io/goliajp/mailrs:$VERSION && docker compose up -d --remove-orphans"
else
  echo "==> rebuilding and restarting container"
  $SSH "cd $REMOTE_DIR && docker compose build --no-cache && docker compose up -d"
fi

# ---------- post-deploy health verify + rollback ----------

if [ "$WEB_ONLY" = false ]; then
  echo "==> waiting for new container to become healthy (timeout ${HEALTH_TIMEOUT_SECS}s)"
  if post_status=$(wait_for_healthy); then
    echo "==> health: $post_status — deploy ok"
  else
    echo "error: post-deploy health check failed: $post_status" >&2

    if [ "$GHCR_MODE" = false ]; then
      echo "==> attempting rollback (legacy binary mode)"

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
    else
      echo "==> ghcr mode rollback requires manual MAILRS_VERSION change in $REMOTE_DIR/.env" >&2
      echo "    1. pick a prior healthy version tag (e.g. \`gh release list -L 5\`)" >&2
      echo "    2. ssh \$SSH_HOST 'sed -i \"s|^MAILRS_VERSION=.*|MAILRS_VERSION=<prev>|\" /apps/mailrs/.env && cd /apps/mailrs && docker pull ghcr.io/goliajp/mailrs:<prev> && docker compose up -d --force-recreate mailrs'" >&2
    fi

    echo "==> container logs (tail 40):"
    $SSH "cd $REMOTE_DIR && docker compose logs --tail 40 mailrs 2>&1" || true
    exit 1
  fi
fi

echo "==> container logs (tail 20):"
$SSH "cd $REMOTE_DIR && docker compose logs --tail 20 mailrs 2>&1" || true

echo "==> deploy complete"
