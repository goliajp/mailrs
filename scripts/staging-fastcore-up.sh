#!/usr/bin/env bash
# staging-fastcore-up.sh — bring up the kevy-backed fastcore binary
# next to the monolith mailrs container on t01.
#
# fastcore is the kevy backend half of the core ↔ fastcore swap charter.
# It listens on :3301 inside the compose network (host-mapped to :3201)
# and exposes the same `mailrs-core-api` surface — limited to the
# Rock 1 + Rock 2 read paths today: conversations:list / categories /
# action-count / unseen-count.
#
# Usage:
#   ./scripts/staging-fastcore-up.sh [--image-tag arch-split-fastcore] [--with-webapi]
#
# --with-webapi: also bring up the shadow webapi pointing at fastcore
#   instead of the monolith core-rpc. Useful for A/B testing the
#   conversation list latency from a real browser session.
#
# Tears down via:
#   ./scripts/staging-fastcore-down.sh

set -euo pipefail

HOST="${STAGING_SSH_HOST:-t01.golia.jp}"
USER="${STAGING_SSH_USER:-root}"
KEY="${STAGING_SSH_KEY:-$HOME/.ssh/id_ed25519}"
IMAGE_TAG="arch-split-fastcore"
WITH_WEBAPI=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --image-tag)   IMAGE_TAG="$2"; shift 2 ;;
        --with-webapi) WITH_WEBAPI=1; shift ;;
        --host)        HOST="$2"; shift 2 ;;
        --user)        USER="$2"; shift 2 ;;
        --key)         KEY="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,22p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1"; exit 2 ;;
    esac
done

SSH="ssh -i $KEY -o StrictHostKeyChecking=no $USER@$HOST"
SCP="scp -i $KEY -o StrictHostKeyChecking=no"

echo "[1/4] syncing docker-compose.staging.split.yml -> $HOST"
$SCP deploy/docker-compose.staging.split.yml \
    "$USER@$HOST:/apps/mailrs-staging/docker-compose.staging.split.yml"

echo "[2/4] pulling ghcr.io/goliajp/mailrs:${IMAGE_TAG}"
$SSH "docker pull ghcr.io/goliajp/mailrs:${IMAGE_TAG}"

echo "[3/4] bringing up fastcore (profile=fastcore)"
$SSH bash <<EOF
set -e
cd /apps/mailrs-staging
grep -q "^MAILRS_VERSION=" .env && sed -i "s/^MAILRS_VERSION=.*/MAILRS_VERSION=${IMAGE_TAG}/" .env || echo "MAILRS_VERSION=${IMAGE_TAG}" >> .env
docker compose -p mailrs-staging -f docker-compose.staging.split.yml \
    --profile fastcore up -d fastcore
sleep 3
echo "--- fastcore /v1/healthz ---"
curl -sf http://127.0.0.1:3201/v1/healthz || echo "(health probe failed)"
echo
EOF

if (( WITH_WEBAPI )); then
    echo "[4/4] bringing up shadow webapi pointing at fastcore"
    $SSH bash <<EOF
set -e
cd /apps/mailrs-staging
# spawn a 2nd webapi instance bound to a different port + secret +
# CORE_RPC_BASE pointing at fastcore. We can't re-use the existing
# split webapi service definition (it points at monolith :3300), so
# launch a one-shot container with overrides.
docker rm -f mailrs-staging-webapi-fc 2>/dev/null || true
docker run -d \
    --name mailrs-staging-webapi-fc \
    --network mailrs-staging_default \
    -p 127.0.0.1:3103:3100 \
    -e MAILRS_CORE_RPC_BASE=http://mailrs:3300 \
    -e MAILRS_FASTCORE_RPC_BASE=http://fastcore:3301 \
    -e MAILRS_CORE_API_SECRET="\$(grep ^MAILRS_CORE_API_SECRET .env | cut -d= -f2)" \
    -e MAILRS_KEVY_URL=kevy://kevy-server:6379 \
    -e MAILRS_WEB_BIND=0.0.0.0:3100 \
    -e MAILRS_WEB_STATIC_DIR=/opt/mailrs/web \
    -e RUST_LOG=info \
    --entrypoint /usr/local/bin/mailrs-webapi \
    ghcr.io/goliajp/mailrs:${IMAGE_TAG}
sleep 3
echo "--- webapi-fc /_health ---"
curl -sf http://127.0.0.1:3103/_health || echo "(probe failed; check logs)"
EOF
else
    echo "[4/4] skipping webapi shadow (omit --with-webapi to keep)"
fi

cat <<EOM

done. fastcore up on $HOST:3201, monolith mailrs untouched on :3101.

direct fastcore curl (SSH tunnel):
  ssh -L 3201:127.0.0.1:3201 $USER@$HOST
  curl http://localhost:3201/v1/healthz
  curl http://localhost:3201/v1/users/lihao@golia.jp/conversations/categories

$( ((WITH_WEBAPI)) && echo "browse webapi via tunnel: ssh -L 3103:127.0.0.1:3103 $USER@$HOST && open http://localhost:3103" )

bring down:
  ./scripts/staging-fastcore-down.sh
EOM
