#!/usr/bin/env bash
# staging-fastcore-down.sh — tear down the fastcore + shadow webapi.
#
# Stops the kevy-backed fastcore container + the standalone
# mailrs-staging-webapi-fc container (if up). Does NOT touch monolith
# mailrs, monolith webapi (profile=split), receiver, or kevy-server.

set -euo pipefail

HOST="${STAGING_SSH_HOST:-t01.golia.jp}"
USER="${STAGING_SSH_USER:-root}"
KEY="${STAGING_SSH_KEY:-$HOME/.ssh/id_ed25519}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host) HOST="$2"; shift 2 ;;
        --user) USER="$2"; shift 2 ;;
        --key)  KEY="$2"; shift 2 ;;
        -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1"; exit 2 ;;
    esac
done

SSH="ssh -i $KEY -o StrictHostKeyChecking=no $USER@$HOST"

echo "stopping fastcore + shadow webapi-fc on $HOST"
$SSH bash <<'EOF'
set -e
cd /apps/mailrs-staging
docker compose -p mailrs-staging -f docker-compose.staging.split.yml \
    --profile fastcore stop fastcore 2>/dev/null || true
docker rm -f mailrs-staging-webapi-fc 2>/dev/null || true
docker ps -a --format '{{.Names}}\t{{.Status}}' | grep -E "mailrs-staging-(fastcore|webapi-fc)" || true
EOF

echo
echo "done. monolith mailrs untouched."
echo "to also drop the kevy-fastcore persist dir (loses test data):"
echo "  $SSH 'rm -rf /var/lib/docker/volumes/mailrs-staging_mailrs-staging-data/_data/kevy-fastcore'"
