#!/usr/bin/env bash
# Build the SQL core's image — fastcore's peer.
#
# `deploy/docker-compose.pg-core.yml` runs this image in fastcore's place.
#
# Usage:
#   ./scripts/build-pg-core.sh                           # PostgreSQL (default)
#   ./scripts/build-pg-core.sh --features core-rpc,spg   # embedded spg
#   ./scripts/build-pg-core.sh --push               # also push to ghcr
#   ./scripts/build-pg-core.sh --tag v2.47.0
#
# Local arm64 build and direct push, matching `direct-deploy.sh` rather than
# CI: everyday releases here do not go through CI,
# and this is not the exception.
set -euo pipefail
cd "$(dirname "$0")/.."

FEATURES="core-rpc"
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
# a release tag is the version the binary reports; anything else is a build
# of no particular release
case "$TAG" in
    v[0-9]*.[0-9]*.[0-9]*|[0-9]*.[0-9]*.[0-9]*) VERSION="${TAG#v}" ;;
    *) VERSION="0.0.0" ;;
esac

echo "building $FULL  (features: $FEATURES)"
docker buildx build \
    --platform linux/arm64 \
    --build-arg "FEATURES=$FEATURES" \
    --build-arg "VERSION=$VERSION" \
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
echo "next: MAILRS_PG_CORE_IMAGE=$FULL in .env, then"
echo "  docker compose -f docker-compose.prod.yml -f docker-compose.pg-core.yml up -d"
echo "and copy existing mail with 'mailrs-core-sync --dry-run' first."
