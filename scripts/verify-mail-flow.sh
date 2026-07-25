#!/usr/bin/env bash
# verify-mail-flow.sh — the gate every deploy must pass.
#
# Mail flow is the one thing that must never break, so it gets an
# instrument rather than a spot-check. Three legs:
#
#   inbound   a spool blob addressed to a real account lands in that
#             account's maildir
#   outbound  a queued job is claimed, DKIM-signed with a d= that
#             matches its From domain, and attempted
#   health    every container up, AOF replay clean, no panics
#
# The outbound leg deliberately does NOT deliver to a real mailbox.
# It targets a domain with no MX, so the job is claimed, signed and
# fails at resolution — which exercises the queue and the signer (both
# run before MX resolution) without mailing a stranger on every run.
# Pass --full to also deliver, to FULL_TARGET.
#
# Usage:
#   scripts/verify-mail-flow.sh              # no side effects worth caring about
#   scripts/verify-mail-flow.sh --full       # also does a real delivery
#
# Exit non-zero, naming the leg, on any failure.

set -uo pipefail

PROD="${PROD:-root@t02.golia.jp}"
INBOUND_ACCOUNT="${INBOUND_ACCOUNT:-dmarc@golia.jp}"
OUTBOUND_FROM_DOMAIN="${OUTBOUND_FROM_DOMAIN:-bitreits.com}"
# RFC 2606 reserved — guaranteed never to resolve to a real MTA.
SINK="probe@sink.invalid"
FULL_TARGET="${FULL_TARGET:-goliaaccess@gmail.com}"
FULL=0
[ "${1:-}" = "--full" ] && FULL=1

STAMP=$(date +%s)
FAILED=()

say()  { printf '%s\n' "$*"; }
ok()   { printf '  ok   %s\n' "$*"; }
bad()  { printf '  FAIL %s\n' "$*"; FAILED+=("$1"); }

# ── leg 1: inbound ────────────────────────────────────────────────
say "[1/3] inbound — spool to maildir"

LOCAL=${INBOUND_ACCOUNT%@*}
DOMAIN=${INBOUND_ACCOUNT#*@}
PROBE="mailflow-in-$STAMP"

python3 - "$INBOUND_ACCOUNT" "$PROBE" <<'PY' > /tmp/mailflow-in.bin
import base64, json, sys, time, email.utils
acct, probe = sys.argv[1], sys.argv[2]
env = {"reverse_path": "noreply@golia.jp", "forward_paths": [acct],
       "is_authenticated": False, "conn_id": 0, "target_folder": "INBOX",
       "received_at": int(time.time()), "schema_version": 1}
body = (f"From: Mail Flow Probe <noreply@golia.jp>\r\nTo: {acct}\r\n"
        f"Subject: {probe}\r\nMessage-ID: <{probe}@golia.jp>\r\n"
        f"Date: {email.utils.formatdate()}\r\n\r\nmail-flow gate\r\n").encode()
blob = b"X-Mailrs-Spool-Envelope: " + base64.b64encode(json.dumps(env).encode()) + b"\r\n" + body
sys.stdout.buffer.write(blob)
PY

base64 < /tmp/mailflow-in.bin | ssh -o ConnectTimeout=20 "$PROD" \
  "base64 -d > /tmp/$PROBE && \
   docker cp /tmp/$PROBE mailrs-fastcore:/data/.spool/incoming/tmp/$PROBE && \
   docker exec -u root mailrs-fastcore sh -c \
     'chown mailrs:mailrs /data/.spool/incoming/tmp/$PROBE && \
      mv /data/.spool/incoming/tmp/$PROBE /data/.spool/incoming/new/$PROBE'" \
  >/dev/null 2>&1 || bad "inbound: could not inject spool blob"

# The drain ticks on an interval; give it a couple of cycles.
for _ in $(seq 1 12); do
    if ssh -o ConnectTimeout=15 "$PROD" \
        "docker exec mailrs-fastcore test -f /data/maildir/$DOMAIN/$LOCAL/new/$PROBE" 2>/dev/null; then
        ok "delivered to $INBOUND_ACCOUNT"
        ssh -o ConnectTimeout=15 "$PROD" \
          "docker exec mailrs-fastcore rm -f /data/maildir/$DOMAIN/$LOCAL/new/$PROBE" >/dev/null 2>&1
        break
    fi
    sleep 5
done
ssh -o ConnectTimeout=15 "$PROD" \
  "docker exec mailrs-fastcore test -f /data/maildir/$DOMAIN/$LOCAL/new/$PROBE" 2>/dev/null \
  && bad "inbound: probe never reached the maildir"

# ── leg 2: outbound ───────────────────────────────────────────────
say "[2/3] outbound — queue claim + DKIM alignment"

TARGET="$SINK"
[ "$FULL" = "1" ] && TARGET="$FULL_TARGET"
OUT_PROBE="mailflow-out-$STAMP"
SENDER="noreply@$OUTBOUND_FROM_DOMAIN"

python3 - "$SENDER" "$TARGET" "$OUT_PROBE" <<'PY' > /tmp/mailflow-out.sh
import base64, json, sys, time, email.utils
sender, target, probe = sys.argv[1], sys.argv[2], sys.argv[3]
now = int(time.time())
msg = (f"From: Mail Flow Probe <{sender}>\r\nTo: {target}\r\n"
       f"Subject: {probe}\r\nMessage-ID: <{probe}@{sender.split('@')[1]}>\r\n"
       f"Date: {email.utils.formatdate()}\r\n\r\nmail-flow gate\r\n").encode()
blob = {"id": 0, "sender": sender, "recipient": target, "original_sender": sender,
        "message_data_b64": base64.b64encode(msg).decode(), "status": "pending",
        "attempts": 0, "last_error": None, "next_retry": None, "scheduled_at": None,
        "created_at": now, "updated_at": now}
b64 = base64.b64encode(json.dumps(blob).encode()).decode()
print('ID=$(docker exec mailrs-kevy kevy-cli INCR "mailrs:outbound:counter" | tr -dc "0-9")')
print(f'BLOB=$(echo {b64} | base64 -d | sed "s/\\"id\\": 0/\\"id\\": $ID/"); '
      f'docker exec mailrs-kevy kevy-cli HSET "mailrs:outbound:job:$ID" '
      f'state pending attempts 0 blob "$BLOB" created_at {now} updated_at {now} >/dev/null && '
      f'docker exec mailrs-kevy kevy-cli LPUSH "mailrs:outbound:pending-idx" "$ID" >/dev/null && '
      f'echo "queued job:$ID"')
PY

JOB_LINE=$(ssh -o ConnectTimeout=25 "$PROD" 'bash -s' < /tmp/mailflow-out.sh 2>/dev/null)
JOB_ID=$(sed -n 's/^queued job:\([0-9]*\)$/\1/p' <<<"$JOB_LINE")
[ -n "$JOB_ID" ] || bad "outbound: could not enqueue"

sleep 8
# The sender writes with ANSI colour codes; strip them before matching
# or `dkim_d=x` never matches (the `=` sits inside an escape sequence).
SENDER_LOG=$(ssh -o ConnectTimeout=20 "$PROD" \
  "docker logs --since 2m mailrs-fastcore-sender 2>&1" 2>/dev/null \
  | sed 's/\x1b\[[0-9;]*m//g')

if grep -q "dkim_d=$OUTBOUND_FROM_DOMAIN" <<<"$SENDER_LOG"; then
    ok "signed d=$OUTBOUND_FROM_DOMAIN (aligned with From)"
else
    bad "outbound: no aligned DKIM signature for $OUTBOUND_FROM_DOMAIN"
fi

if grep -qE "delivering .*$OUTBOUND_FROM_DOMAIN" <<<"$SENDER_LOG"; then
    ok "job claimed from the queue"
else
    bad "outbound: sender never claimed the job"
fi

if [ "$FULL" = "1" ]; then
    grep -q "delivered .*code=250" <<<"$SENDER_LOG" \
      && ok "delivered to $FULL_TARGET" \
      || bad "outbound: full delivery did not complete"
fi

# A no-MX target fails transiently, so the job requeues and would
# retry for hours. Drop the job hash: the id stays in pending-idx but
# the claim finds no state=pending and skips it, which is the same
# path duplicate ids already take.
if [ -n "$JOB_ID" ]; then
    ssh -o ConnectTimeout=15 "$PROD" \
      "docker exec mailrs-kevy kevy-cli DEL 'mailrs:outbound:job:$JOB_ID'" \
      >/dev/null 2>&1
fi

# ── leg 3: health ─────────────────────────────────────────────────
say "[3/3] health — containers, replay, panics"

# No `bc` on the host — sum in the shell.
HEALTH=$(ssh -o ConnectTimeout=20 "$PROD" '
  echo "CONTAINERS=$(docker ps --format "{{.Names}}" | grep -c "^mailrs-")"
  echo "REPLAY=$(docker logs mailrs-fastcore 2>&1 | grep -c "(clean)")"
  total=0
  for c in mailrs-fastcore mailrs-fastcore-sender mailrs-receiver mailrs-webapi-fc; do
    n=$(docker logs --since 10m $c 2>&1 | grep -ci panic)
    total=$((total + n))
  done
  echo "PANIC=$total"
' 2>/dev/null)

CONTAINERS=$(sed -n 's/^CONTAINERS=//p' <<<"$HEALTH")
REPLAY=$(sed -n 's/^REPLAY=//p' <<<"$HEALTH")
PANIC=$(sed -n 's/^PANIC=//p' <<<"$HEALTH")

[ "${CONTAINERS:-0}" -ge 5 ] && ok "containers up: $CONTAINERS" \
                             || bad "health: only ${CONTAINERS:-0} containers up"
[ "${REPLAY:-0}" -ge 1 ]     && ok "AOF replay clean" \
                             || bad "health: no clean replay line"
[ "${PANIC:-1}" -eq 0 ]      && ok "no panics" \
                             || bad "health: ${PANIC} panic lines"

# ── verdict ───────────────────────────────────────────────────────
echo
if [ ${#FAILED[@]} -eq 0 ]; then
    say "MAIL FLOW OK"
    exit 0
fi
say "MAIL FLOW FAILED:"
printf '  - %s\n' "${FAILED[@]}"
exit 1
