#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cleanup() {
  echo ""
  echo "==> shutting down"
  kill $PID_CARGO $PID_VITE 2>/dev/null || true
  wait $PID_CARGO $PID_VITE 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# load secrets from .env.local
if [ -f "$ROOT/.env.local" ]; then
  set -a
  source "$ROOT/.env.local"
  set +a
fi

export MAILRS_HOSTNAME=localhost
export MAILRS_MAILDIR=/tmp/mailrs/maildir
export MAILRS_WEB_PORT=3200
export MAILRS_PG_URL=postgres://mailrs:mailrs@localhost:5432/mailrs
export MAILRS_KEVY_URL=redis://localhost:6379
export MAILRS_LOCAL_DOMAINS=localhost,golia.jp
export MAILRS_USERS_FILE=/tmp/mailrs/users.toml
export MAILRS_DNSBL_ENABLED=false
export MAILRS_ANTISPAM_ENABLED=false
export MAILRS_AI_ANALYSIS_ENABLED=true

mkdir -p /tmp/mailrs/maildir

export MAILRS_IMAPS_PORT=1993

echo "==> starting cargo run (SMTP 2525, submission 2587, IMAP 1143, IMAPS 1993, web API 3200)"
cargo run --bin mailrs-server &
PID_CARGO=$!

echo "==> starting vite dev server (http://localhost:5173)"
(cd web && bunx --bun vite) &
PID_VITE=$!

wait
