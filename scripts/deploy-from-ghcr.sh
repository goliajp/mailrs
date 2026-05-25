#!/usr/bin/env bash
# usage: ./scripts/deploy-from-ghcr.sh [<tag>]
#
# v5 remote-host deploy script. Pulls the named image from ghcr.io and
# brings the compose stack up. Replaces the cross-compile-then-ssh-binary
# flow in scripts/release.sh once v5.3 cutover completes.
#
# Defaults to whatever MAILRS_VERSION says in the host's .env. Pass an
# explicit tag to override (useful for rollback).
#
# Pre-reqs on the remote host (one-time):
#   - docker + docker-compose-v2 installed
#   - repo cloned at $REPO_DIR
#   - .env populated (MAILRS_VERSION, MAILRS_PG_PASSWORD, etc.)
#   - logged in to ghcr.io (`docker login ghcr.io -u <user> -p <PAT>`)
#     or running on a host with image-pull permission (private images
#     need a PAT with read:packages)

set -euo pipefail

REPO_DIR="${REPO_DIR:-/apps/mailrs}"
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.prod.yml}"

cd "$REPO_DIR"

TAG="${1:-}"
if [ -n "$TAG" ]; then
  echo "==> deploying mailrs:$TAG (override)"
  export MAILRS_VERSION="$TAG"
else
  # shellcheck disable=SC1091
  source .env
  echo "==> deploying mailrs:${MAILRS_VERSION:-latest} (from .env)"
fi

echo "==> pulling image"
docker compose -f "$COMPOSE_FILE" pull mailrs

echo "==> rolling restart"
docker compose -f "$COMPOSE_FILE" up -d --no-deps mailrs

echo "==> waiting for health (timeout 60s)"
deadline=$((SECONDS + 60))
while [ "$SECONDS" -lt "$deadline" ]; do
  status=$(docker inspect -f '{{.State.Health.Status}}' mailrs 2>/dev/null || echo "unknown")
  if [ "$status" = "healthy" ]; then
    echo "==> health: healthy — deploy ok"
    exit 0
  fi
  sleep 2
done

echo "==> ERROR: health check timed out (status=$status)"
echo "==> last 30 log lines:"
docker logs --tail 30 mailrs
exit 1
