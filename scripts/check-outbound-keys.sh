#!/usr/bin/env bash
# No file may spell an outbound queue key it does not own.
set -euo pipefail
cd "$(dirname "$0")/.."
owner=crates/core-sidestate/src/families/outbound/mod.rs
hits=$(grep -rn 'b"mailrs:outbound:\(pending-idx\|scheduled-idx\|scheduled\)"' crates \
  --include='*.rs' | grep -v "^$owner:" || true)
if [ -n "$hits" ]; then
  echo "!! these spell an outbound queue key instead of importing it:"
  echo "$hits"
  echo "   use mailrs_core_sidestate::families::outbound::{PENDING_IDX, SCHEDULED_IDX}"
  exit 1
fi
echo "outbound queue keys: one definition"
