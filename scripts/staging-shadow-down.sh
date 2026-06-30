#!/usr/bin/env bash
# staging-shadow-down.sh — tear down the 4-process split shadow on t01.
#
# Stops the webapi + sender containers (profile=split), but DOES NOT
# touch the monolith mailrs container. Idempotent.
#
# Usage:
#   ./scripts/staging-shadow-down.sh [--rm]
#
#   --rm   also `docker compose rm -f` the containers (default: leave
#          them stopped so logs are still inspectable)
#
# Pairs with staging-shadow-up.sh.

set -euo pipefail

HOST="${STAGING_SSH_HOST:-t01.golia.jp}"
USER="${STAGING_SSH_USER:-root}"
KEY="${STAGING_SSH_KEY:-$HOME/.ssh/id_ed25519}"
DO_RM=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --rm)   DO_RM=1; shift ;;
        --host) HOST="$2"; shift 2 ;;
        --user) USER="$2"; shift 2 ;;
        --key)  KEY="$2"; shift 2 ;;
        -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1"; exit 2 ;;
    esac
done

SSH="ssh -i $KEY -o StrictHostKeyChecking=no $USER@$HOST"

echo "stopping webapi + sender on $HOST"
$SSH bash <<EOF
set -e
cd /apps/mailrs-staging
docker compose -p mailrs-staging -f docker-compose.staging.split.yml \
    --profile split stop webapi sender 2>/dev/null || true
if [ "$DO_RM" = "1" ]; then
    docker compose -p mailrs-staging -f docker-compose.staging.split.yml \
        --profile split rm -f webapi sender 2>/dev/null || true
fi
docker ps -a --format '{{.Names}}\t{{.Status}}' | grep mailrs-staging || true
EOF

echo
echo "done. monolith mailrs untouched."
echo "to also wipe MAILRS_CORE_API_SECRET from .env:"
echo "  $SSH 'sed -i /^MAILRS_CORE_API_SECRET/d /apps/mailrs-staging/.env'"
