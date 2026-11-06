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
#
# The account purge registry (REGISTRY) lists every family an account owns,
# by name, so it spells prefixes too; it is exempt here, and
# `check-user-keys.py` (run last) holds it to the opposite rule: every family
# below, and every key built from an address, must have an entry there.
set -euo pipefail
cd "$(dirname "$0")/.."

# prefix (ERE, matched right after the opening quote) ; defining file
FAMILIES='
mailrs:outbound:;crates/core-sidestate/src/families/outbound/keys.rs
outbound:(paused|held|rate|fail|bounce|complaint);crates/core-sidestate/src/families/outbound/pause.rs
mailrs:user:[^"]*:contacts;crates/core-sidestate/src/families/contact_cards.rs
mailrs:contact:;crates/core-sidestate/src/families/contacts.rs
mailrs:imap-import:;crates/core-sidestate/src/families/imap_import.rs
mailrs:masked:;crates/core-sidestate/src/families/masked_email/mod.rs
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
admin:rbl:status;crates/core-sidestate/src/families/egress.rs
egress:refusals:;crates/core-sidestate/src/families/egress.rs
(drafts|signatures|templates):;crates/core-sidestate/src/families/prefs.rs
inline:;crates/core-sidestate/src/families/inline_images.rs
calendar_events?:;crates/core-sidestate/src/families/calendar_events/mod.rs
push:tokens:;crates/core-sidestate/src/keys/push.rs
push:(web|unifiedpush|used|removed|session|upwindow|uptrailing):;crates/core-sidestate/src/keys/push.rs
shared:;crates/core-sidestate/src/families/shared_inbox/mod.rs
session:;crates/core-sidestate/src/keys/session.rs
spam:[^"]*:(whitelist|blacklist);crates/core-sidestate/src/families/sender_lists.rs
sieve:;crates/core-sidestate/src/lib.rs
saved_searches:;crates/core-sidestate/src/families/saved_searches.rs
rules:;crates/core-sidestate/src/families/rules.rs
forwarding:;crates/core-sidestate/src/families/forwarding.rs
owner-notices;crates/core-sidestate/src/families/owner_notices.rs
vacation:;crates/core-sidestate/src/families/vacation.rs
swipe:;crates/core-sidestate/src/families/swipe.rs
devices:;crates/core-sidestate/src/families/devices.rs
mailrs:domfirst;crates/core-sidestate/src/families/domain_first.rs
mailrs:fraud:hits:;crates/core-sidestate/src/families/fraud_hits.rs
mailrs:recalled:;crates/core-sidestate/src/families/recall_notices.rs
mailrs:submitted:;crates/core-sidestate/src/families/submitted.rs
mailrs:report(s|-outcome|s-pending)?[:"];crates/core-sidestate/src/families/reports.rs
rep:obs;crates/core-sidestate/src/families/screening.rs
scan:hashobs;crates/core-sidestate/src/families/screening.rs
screening:rules;crates/core-sidestate/src/families/screening.rs
gl:;crates/shield/src/greylist_key.rs
ag:;crates/auth-guard/src/login.rs
auth:events:;crates/auth-guard/src/events.rs
app_pw:;crates/auth-guard/src/app_passwords.rs
auth:alerts:;crates/auth-guard/src/alerts.rs
notify:new-mail;crates/core/src/event_bus.rs
notify:reputation;crates/core-sidestate/src/families/screening.rs
notify:account-signout;crates/core-sidestate/src/keys/signout.rs
agent:keys:;crates/core-sidestate/src/keys/credentials.rs
totp:;crates/core-sidestate/src/keys/credentials.rs
pwreset(_by_addr)?:;crates/core-sidestate/src/keys/credentials.rs
reset:code:;crates/core-sidestate/src/keys/reset.rs
reset:failures:;crates/core-sidestate/src/keys/reset.rs
bayes:(v2:)?(tokens|meta|corpus|trained);crates/triage/src/spam.rs
bayes:triage:;crates/triage/src/lib.rs
mailrs:purge:;crates/core-sidestate/src/families/account_purge/pending.rs
'

REGISTRY=crates/core-sidestate/src/families/account_purge/registry.rs

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
    | grep -v "^$owner:" | grep -v "^$REGISTRY:" || true)
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

printf '%s\n' "$FAMILIES" | grep ';' | cut -d';' -f1 | python3 scripts/check-user-keys.py || fail=1

[ "$fail" = 1 ] && exit 1

n=$(printf '%s\n' "$FAMILIES" | grep -c ';')
echo "shared keys: $n families, each spelled only in its defining file"
