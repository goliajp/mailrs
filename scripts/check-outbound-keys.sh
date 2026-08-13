#!/usr/bin/env bash
# Shared keys: one spelling each.
#
# Two shapes of the same rule, because the crate graph allows one and not the
# other:
#
#   OUTBOUND — every crate that needs these can import them, so spelling one
#   at all is the error.
#
#   SIEVE — `mailrs-core-sidestate` is an OPTIONAL dependency of
#   `mailrs-server` (gated on `core-rpc`), so the unconditional code there
#   cannot import from it, and making the dep unconditional would change the
#   dependency graph of an artifact meant to be unaffected by the feature. Two
#   definitions are therefore unavoidable — so what is checked is that every
#   spelling in the tree is byte-identical.
#
# Both exist because agreement is not the same as having one definition. The
# outbound half of this script found eight more hand-spelled copies the day it
# was written, all of them still agreeing; the sieve key had NINE across five
# files, and a script saved in the UI filtered no mail on the SQL lane because
# one reader looked somewhere else entirely.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0

owner=crates/core-sidestate/src/families/outbound/mod.rs
hits=$(grep -rn 'b"mailrs:outbound:\(pending-idx\|scheduled-idx\|scheduled\)"' crates \
  --include='*.rs' | grep -v "^$owner:" || true)
if [ -n "$hits" ]; then
  echo "!! these spell an outbound queue key instead of importing it:"
  echo "$hits"
  echo "   use mailrs_core_sidestate::families::outbound::{PENDING_IDX, SCHEDULED_IDX}"
  fail=1
fi

# Every construction of the sieve key, and the one form all of them must take.
want='format!("sieve:{address}")'
spellings=$(grep -rn --include='*.rs' -o 'format!("sieve:[^"]*")' crates || true)
odd=$(printf '%s\n' "$spellings" | grep -v "$want\$" || true)
if [ -n "$odd" ]; then
  echo "!! these spell the sieve key differently:"
  printf '%s\n' "$odd" | sed 's/^/    /'
  echo "   every spelling must be exactly: $want"
  echo "   crates that can import it use mailrs_core_sidestate::sieve_key"
  fail=1
fi

[ "$fail" = 1 ] && exit 1

n_sieve=$(printf '%s\n' "$spellings" | grep -c . || true)
echo "shared keys: outbound has one definition; sieve spelled $n_sieve time(s), all identical"
