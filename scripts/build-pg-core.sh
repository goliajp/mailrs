#!/usr/bin/env bash
# Build the SQL core's image — fastcore's peer.
#
# This exists because `deploy/docker-compose.split.yml` referenced an image
# nobody built. The service was written for `mailrs-server`, the fat binary the
# main image stopped shipping in July, so the switch it describes could not be
# performed at all: not dormant, undeployable. An image name in a compose file
# is not a build.
#
# Usage:
#   ./scripts/build-pg-core.sh                      # spg-embedded (default)
#   ./scripts/build-pg-core.sh --features core-rpc  # real PostgreSQL
#   ./scripts/build-pg-core.sh --push               # also push to ghcr
#   ./scripts/build-pg-core.sh --tag v2.47.0
#
# Local arm64 build and direct push, matching `direct-deploy.sh` rather than
# CI: everyday releases here do not go through CI (rules/dev-deploy-workflow.md),
# and this is not the exception.
set -euo pipefail
cd "$(dirname "$0")/.."

FEATURES="core-rpc,spg"
TAG="latest"
PUSH=0
IMAGE="ghcr.io/goliajp/mailrs-pg-core"

while [ $# -gt 0 ]; do
    case "$1" in
        --features) FEATURES="$2"; shift 2 ;;
        --tag)      TAG="$2"; shift 2 ;;
        --push)     PUSH=1; shift ;;
        -h|--help)
            sed -n '2,20p' "$0" | sed 's/^# \?//'
            exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
done

# Fail before the build rather than after it, and say which one is wrong: a
# typo here produces an image whose backend is not the one the operator meant,
# and both spellings compile.
case "$FEATURES" in
    core-rpc|core-rpc,spg) ;;
    *)
        echo "!! --features must be 'core-rpc' (real PostgreSQL) or"
        echo "   'core-rpc,spg' (spg-embedded). Got: $FEATURES"
        exit 1 ;;
esac

# The feature set is in the tag. Two images that differ only in which SQL
# engine they talk to, sharing one tag, is a switch nobody can audit after the
# fact — `docker inspect` cannot tell you which cargo features built a binary.
case "$FEATURES" in
    core-rpc,spg) SUFFIX="-spg" ;;
    core-rpc)     SUFFIX="-pg" ;;
esac
FULL="${IMAGE}:${TAG}${SUFFIX}"

echo "building $FULL  (features: $FEATURES)"
docker buildx build \
    --platform linux/arm64 \
    --build-arg "FEATURES=$FEATURES" \
    -f deploy/Dockerfile.pg-core \
    -t "$FULL" \
    --load \
    .

echo
echo "built: $FULL"
docker run --rm --entrypoint sh "$FULL" -c 'ls -l /usr/local/bin/' | sed 's/^/    /'

# Prove the binary runs and refuses cleanly without its backend, rather than
# only that it linked. A binary that panics on startup passes every build check
# there is.
echo
echo "startup check (expected: a clear complaint about the missing backend)"
docker run --rm "$FULL" 2>&1 | head -3 | sed 's/^/    /' || true

if [ "$PUSH" = 1 ]; then
    echo
    echo "pushing $FULL"
    docker push "$FULL"
fi

echo
echo "next: set the split compose's image to $FULL, bring it up alongside the"
echo "running core, and follow deploy/dual-mode-switch.md — which starts with"
echo "'mailrs-core-sync --dry-run', not with the sync."
