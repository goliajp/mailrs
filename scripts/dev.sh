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
# spg-embedded — in-process SQL engine; no postgres container needed.
# Fresh catalog bootstraps from init-schema.sql via `spg import`
# (spgctl binary from the goliajp/spg checkout or PATH).
export MAILRS_PG_URL=spg:///tmp/mailrs/spg/mailrs.spg
# MAILRS_KEVY_URL intentionally unset: local dev runs kevy in-process
# (Phase C). Set it only when running a real kevy-server for the
# receiver-split topology (P6), as kevy://host:port — not redis://.
export MAILRS_LOCAL_DOMAINS=localhost,golia.jp
export MAILRS_USERS_FILE=/tmp/mailrs/users.toml
export MAILRS_DNSBL_ENABLED=false
export MAILRS_ANTISPAM_ENABLED=false
export MAILRS_AI_ANALYSIS_ENABLED=true

mkdir -p /tmp/mailrs/maildir /tmp/mailrs/spg

if [ ! -f /tmp/mailrs/spg/mailrs.spg ]; then
  SPG_BIN="$(command -v spg || true)"
  if [ -z "$SPG_BIN" ]; then
    for cand in "$ROOT/../../goliajp/spg/target/release/spg" "$ROOT/../../goliajp/spg/target/debug/spg"; do
      [ -x "$cand" ] && SPG_BIN="$cand" && break
    done
  fi
  if [ -z "$SPG_BIN" ]; then
    echo "error: fresh spg catalog needs the 'spg' CLI (cargo build -p spgctl in goliajp/spg)" >&2
    exit 1
  fi
  echo "==> bootstrapping spg catalog from scripts/init-schema.sql"
  "$SPG_BIN" import --db /tmp/mailrs/spg/mailrs.spg --file "$ROOT/scripts/init-schema.sql"
fi

export MAILRS_IMAPS_PORT=1993

echo "==> starting cargo run (SMTP 2525, submission 2587, IMAP 1143, IMAPS 1993, web API 3200)"
cargo run --bin mailrs-server --features spg &
PID_CARGO=$!

echo "==> starting vite dev server (http://localhost:5173)"
(cd web && bunx --bun vite) &
PID_VITE=$!

wait
