#!/usr/bin/env bash
# Shared kevy keys: one spelling each.
#
# Several processes (receiver, fastcore, the sender, webapi) and the crates
# they share read and write the same network-kevy keys. A key spelled in two
# places can drift, and then one side writes where the other never reads —
# no error anywhere, the feature just does nothing. This happened: the
# sender's due-sweep carried `mailrs:outbound:scheduled` while every writer
# said `...scheduled-idx`, so no scheduled send was ever promoted.
#
# So each family below has one defining file. A string literal that starts
# with the family's prefix anywhere else in crates/ fails the check; import
# the definition instead. Comment lines are ignored. EXCEPTIONS lists the
# few spellings that cannot import, each with its reason.
set -euo pipefail
cd "$(dirname "$0")/.."

# prefix (ERE, matched right after the opening quote) ; defining file
FAMILIES='
mailrs:outbound:;crates/core-sidestate/src/families/outbound/keys.rs
mailrs:user:[^"]*:contacts;crates/core-sidestate/src/families/contact_cards.rs
mailrs:contact:;crates/core-sidestate/src/families/contacts.rs
mailrs:quota:;crates/core-sidestate/src/keys/quota.rs
mailrs:dmarcrpt:;crates/core-sidestate/src/keys/dmarc.rs
mailrs:events:notify:;crates/core-sidestate/src/keys/events.rs
mailrs:bounce:;crates/intake/src/notice.rs
mailrs:mock:;crates/core-sidestate/src/families/mock/mod.rs
mailrs:forward:local:;crates/core-sidestate/src/families/local_forward.rs
mailrs:alias:v2:;crates/alias-store/src/keys.rs
aliases_by_(domain|target);crates/alias-store/src/keys.rs
admin:(audit_log|system-config|greylist:local-lists);crates/core-sidestate/src/keys/admin.rs
admin:groups;crates/core-sidestate/src/families/permission_groups.rs
(drafts|signatures|templates):;crates/core-sidestate/src/families/prefs.rs
calendar_events?:;crates/core-sidestate/src/families/calendar_events/mod.rs
push:tokens:;crates/core-sidestate/src/keys/push.rs
session:;crates/core-sidestate/src/keys/session.rs
spam:[^"]*:(whitelist|blacklist);crates/core-sidestate/src/families/sender_lists.rs
sieve:;crates/core-sidestate/src/lib.rs
gl:;crates/shield/src/greylist_key.rs
ag:;crates/auth-guard/src/login.rs
auth:events:;crates/auth-guard/src/events.rs
auth:alerts:;crates/auth-guard/src/alerts.rs
notify:new-mail;crates/core/src/event_bus.rs
notify:account-signout;crates/core-sidestate/src/keys/signout.rs
bayes:(tokens|meta|corpus|trained);crates/triage/src/spam.rs
bayes:triage:;crates/triage/src/lib.rs
'

# file ; literal prefix ; why it cannot import
EXCEPTIONS='
crates/webapi/src/handlers/smtp_monitor.rs;notify:new-mail;a test feeding a channel the monitor must ignore; webapi does not depend on mailrs-core (it pulls ldap3)
crates/shield/tests/kevy_server_spike.rs;notify:new-mail;exercises kevy pub/sub with the real channel name; the test does not link mailrs-core
'

fail=0
while IFS=';' read -r prefix owner; do
  [ -z "$prefix" ] && continue
  # a pattern that matches nothing, not even its definition, checks nothing
  if ! grep -qE "b?\"($prefix)[^ ]" "$owner"; then
    echo "!! \"$prefix\" is not spelled in its defining file $owner"
    fail=1
  fi
  hits=$(grep -rnE --include='*.rs' "b?\"($prefix)[^ ]" crates \
    | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' \
    | grep -v "^$owner:" || true)
  while IFS=';' read -r file lit _why; do
    [ -z "$file" ] && continue
    hits=$(printf '%s\n' "$hits" | awk -v f="$file:" -v l="\"$lit" \
      'index($0, f) != 1 || index($0, l) == 0' || true)
  done <<< "$EXCEPTIONS"
  if [ -n "$hits" ]; then
    echo "!! these spell a \"$prefix\" key instead of importing it from $owner:"
    printf '%s\n' "$hits" | sed 's/^/    /'
    fail=1
  fi
done <<< "$FAMILIES"

[ "$fail" = 1 ] && exit 1

n=$(printf '%s\n' "$FAMILIES" | grep -c ';')
echo "shared keys: $n families, each spelled only in its defining file"
