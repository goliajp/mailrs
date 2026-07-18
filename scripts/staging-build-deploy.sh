#!/usr/bin/env bash
# staging-build-deploy.sh — the daily dev loop, zero CI involved.
#
#   1. build the full image LOCALLY (docker buildx, native arm64)
#   2. ship it to t01 over ssh (docker save | load — no registry)
#   3. roll the staging fastcore stack + sync the soak harness
#   4. stamp the deployed commit sha + kick the 30-min soak gate
#
# Prod stays CI-owned: when staging has soaked green on a commit, cut a
# v* tag from that SAME commit and release.yml checks the verdict sha
# before deploying to t02. See .claude/rules/dev-deploy-workflow.md.
#
# Usage:
#   ./scripts/staging-build-deploy.sh              # clean tree required
#   ./scripts/staging-build-deploy.sh --allow-dirty  # deploy uncommitted
#     work (soak sha NOT stamped — a dirty build can never gate a tag)
#   SKIP_BUILD=1 ./scripts/staging-build-deploy.sh  # reuse last local image
set -euo pipefail

HOST="${STAGING_HOST:-t01}"
TAG="mailrs:staging-local"
REMOTE_TAG="ghcr.io/goliajp/mailrs:staging-local"
ALLOW_DIRTY=0
[ "${1:-}" = "--allow-dirty" ] && ALLOW_DIRTY=1

cd "$(git rev-parse --show-toplevel)"

DIRTY=0
if ! git diff --quiet || ! git diff --cached --quiet; then
    DIRTY=1
    if [ "$ALLOW_DIRTY" != 1 ]; then
        echo "working tree dirty — commit first, or pass --allow-dirty" >&2
        echo "(dirty builds deploy fine but never stamp the soak-gate sha)" >&2
        exit 1
    fi
fi
SHA=$(git rev-parse HEAD)
SHORT=${SHA:0:8}
VERSION="0.0.0-staging+${SHORT}$( [ "$DIRTY" = 1 ] && echo .dirty || true )"

if [ "${SKIP_BUILD:-0}" != 1 ]; then
    echo "==> [1/4] local image build ($VERSION)"
    docker buildx build \
        --platform linux/arm64 \
        --build-arg VERSION="$VERSION" \
        -t "$TAG" \
        --load \
        .
else
    echo "==> [1/4] SKIP_BUILD=1 — reusing local $TAG"
fi

echo "==> [2/4] shipping image to $HOST (save | ssh load)"
docker save "$TAG" | gzip -1 | ssh "$HOST" 'gunzip | docker load'
ssh "$HOST" "docker tag $TAG $REMOTE_TAG"

echo "==> [3/4] syncing compose + soak harness, rolling the stack"
scp -q deploy/docker-compose.staging.yml "$HOST":/apps/mailrs-staging/docker-compose.staging.yml
scp -q scripts/check-staging-gate.sh scripts/staging-soak-gate.sh scripts/staging-traffic-gen.sh \
    "$HOST":/usr/local/bin/
scp -q deploy/staging-soak-gate.service deploy/staging-traffic-gen.service \
    "$HOST":/etc/systemd/system/
ssh "$HOST" 'chmod +x /usr/local/bin/check-staging-gate.sh /usr/local/bin/staging-soak-gate.sh /usr/local/bin/staging-traffic-gen.sh
systemctl daemon-reload
cd /apps/mailrs-staging
grep -q "^MAILRS_VERSION=" .env && sed -i "s/^MAILRS_VERSION=.*/MAILRS_VERSION=staging-local/" .env || echo "MAILRS_VERSION=staging-local" >> .env
docker compose -p mailrs-staging -f docker-compose.staging.yml up -d --remove-orphans'

echo "==> [4/4] health gate"
ssh "$HOST" 'for i in $(seq 1 36); do
  ok=1
  curl -fsS --max-time 5 http://127.0.0.1:3201/v1/healthz 2>/dev/null | grep -q "\"backend\":\"kevy\"" || ok=0
  [ "$ok" = 1 ] && curl -fsS --max-time 5 http://127.0.0.1:3103/_health >/dev/null 2>&1 || ok=0
  if [ "$ok" = 1 ]; then echo "staging healthy (:3201 + :3103, attempt $i/36)"; exit 0; fi
  echo "  health attempt $i/36 not ready yet"
  sleep 5
done
echo "health check FAILED" >&2
docker logs --tail 30 mailrs-staging-fastcore >&2 || true
exit 1'

if [ "$DIRTY" = 1 ]; then
    echo "==> dirty build — soak-gate sha NOT stamped (tag-gating impossible for uncommitted work)"
else
    echo "==> stamping sha $SHORT + kicking 30-min soak gate"
    ssh "$HOST" "echo '$SHA' > /etc/staging-deploy-sha
systemctl reset-failed staging-traffic-gen.service 2>/dev/null || true
systemctl restart staging-traffic-gen.service
systemctl reset-failed staging-soak-gate.service 2>/dev/null || true
systemctl restart --no-block staging-soak-gate.service"
    echo "    verdict lands in ~35 min: ssh $HOST cat /var/run/staging-gate.json"
    echo "    when pass=true → git flow release v<X.Y.Z> from THIS commit → CI deploys prod"
fi
echo "done: staging on $VERSION"
