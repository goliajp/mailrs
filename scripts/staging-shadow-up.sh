#!/usr/bin/env bash
# staging-shadow-up.sh — bring up the 4-process split shadow on t01.
#
# Run from your laptop. Requires t01 SSH access. Does NOT touch the
# monolith mailrs container — webapi + sender boot in `profile=split`
# alongside it. Sender is left DOWN by default (set --with-sender to
# bring it up; then disable the monolith's sender thread first or you
# WILL get double-deliveries).
#
# Usage:
#   ./scripts/staging-shadow-up.sh [--image-tag arch-split-fastcore] [--with-sender]
#
# Effects:
#   - scp deploy/docker-compose.staging.split.yml to t01
#   - ensure MAILRS_CORE_API_SECRET exists in /apps/mailrs-staging/.env
#     (generated if absent; ~/.cache/mailrs-staging-secret cached)
#   - docker pull ghcr.io/goliajp/mailrs:<tag>
#   - docker compose up -d (mailrs reload to pick up RPC env vars)
#   - docker compose --profile split up -d webapi [sender]
#   - curl http://127.0.0.1:3102/_health
#
# When done, browse via SSH tunnel:
#   ssh -L 3102:127.0.0.1:3102 root@t01.golia.jp
#   open http://localhost:3102

set -euo pipefail

HOST="${STAGING_SSH_HOST:-t01.golia.jp}"
USER="${STAGING_SSH_USER:-root}"
KEY="${STAGING_SSH_KEY:-$HOME/.ssh/id_ed25519}"
IMAGE_TAG="arch-split-fastcore"
WITH_SENDER=0
SKIP_CORE_RECREATE=0
ASSUME_YES=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --image-tag)   IMAGE_TAG="$2"; shift 2 ;;
        --with-sender) WITH_SENDER=1; shift ;;
        --skip-core)   SKIP_CORE_RECREATE=1; shift ;;
        --yes|-y)      ASSUME_YES=1; shift ;;
        --host)        HOST="$2"; shift 2 ;;
        --user)        USER="$2"; shift 2 ;;
        --key)         KEY="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,30p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1"; exit 2 ;;
    esac
done

SSH="ssh -i $KEY -o StrictHostKeyChecking=no $USER@$HOST"
SCP="scp -i $KEY -o StrictHostKeyChecking=no"

echo "[1/5] syncing docker-compose.staging.split.yml -> $HOST"
$SCP deploy/docker-compose.staging.split.yml \
    "$USER@$HOST:/apps/mailrs-staging/docker-compose.staging.split.yml"

echo "[2/5] ensuring MAILRS_CORE_API_SECRET on $HOST"
$SSH bash <<EOF
set -e
cd /apps/mailrs-staging
if ! grep -q "^MAILRS_CORE_API_SECRET=" .env 2>/dev/null; then
    SECRET=\$(openssl rand -hex 32)
    echo "MAILRS_CORE_API_SECRET=\$SECRET" >> .env
    echo "  generated new secret"
else
    echo "  existing secret preserved"
fi
# overwrite MAILRS_VERSION so the monolith picks up the same image
if grep -q "^MAILRS_VERSION=" .env; then
    sed -i "s/^MAILRS_VERSION=.*/MAILRS_VERSION=${IMAGE_TAG}/" .env
else
    echo "MAILRS_VERSION=${IMAGE_TAG}" >> .env
fi
EOF

echo "[3/5] pulling ghcr.io/goliajp/mailrs:${IMAGE_TAG}"
$SSH "docker pull ghcr.io/goliajp/mailrs:${IMAGE_TAG}"

if (( SKIP_CORE_RECREATE )); then
    echo "[4/5] skipping monolith recreate (assuming RPC already up on :3300)"
    if ! $SSH "docker exec mailrs-staging curl -sf -o /dev/null \
            -H 'Authorization: Bearer '\$(grep ^MAILRS_CORE_API_SECRET /apps/mailrs-staging/.env | cut -d= -f2) \
            http://127.0.0.1:3300/v1/healthz"; then
        echo "ERROR: core RPC :3300 not reachable. Re-run without --skip-core."
        exit 1
    fi
    echo "  RPC pre-check ok"
else
    if (( ! ASSUME_YES )); then
        echo
        echo "  About to RECREATE the monolith mailrs container on $HOST."
        echo "  This drops :3101 + IMAP/POP3/sieve for ~30s while it boots back up."
        echo "  Use --skip-core to skip this step on re-runs, or --yes to bypass this prompt."
        read -rp "  Continue? [y/N] " ans
        [[ "$ans" =~ ^[yY] ]] || { echo "aborted by user"; exit 1; }
    fi
    echo "[4/5] recreating mailrs (core) to pick up MAILRS_CORE_RPC_ADDR + SECRET"
    $SSH bash <<EOF
set -e
cd /apps/mailrs-staging
docker compose -p mailrs-staging -f docker-compose.staging.split.yml up -d --no-deps mailrs
# wait for core RPC listener
for i in \$(seq 1 24); do
    if docker exec mailrs-staging curl -sf -o /dev/null \
            -H "Authorization: Bearer \$(grep ^MAILRS_CORE_API_SECRET .env | cut -d= -f2)" \
            http://127.0.0.1:3300/v1/healthz; then
        echo "  core RPC :3300 ready"
        break
    fi
    sleep 5
done
EOF
fi

echo "[5/5] bringing up webapi $( ((WITH_SENDER)) && echo "+ sender" )"
if (( WITH_SENDER )); then
    SERVICES="webapi sender"
else
    SERVICES="webapi"
fi
$SSH bash <<EOF
set -e
cd /apps/mailrs-staging
docker compose -p mailrs-staging -f docker-compose.staging.split.yml \
    --profile split up -d $SERVICES
sleep 3
echo "--- webapi /_health ---"
curl -sf http://127.0.0.1:3102/_health || echo "(health probe failed; check 'docker logs mailrs-staging-webapi')"
echo
echo "--- container status ---"
docker ps --format '{{.Names}}\t{{.Status}}' | grep mailrs-staging
EOF

cat <<EOM

done. shadow webapi up on $HOST:3102.

next:
  ssh -L 3102:127.0.0.1:3102 $USER@$HOST
  open http://localhost:3102

bring down:
  $SSH 'cd /apps/mailrs-staging && docker compose -p mailrs-staging \\
      -f docker-compose.staging.split.yml --profile split stop webapi $((WITH_SENDER)) && echo sender'
EOM
