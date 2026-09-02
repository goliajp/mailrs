#!/usr/bin/env bash
# probe-smtp-monitor.sh — does the SMTP live monitor actually carry a session?
#
# The monitor's stream crosses two processes over kevy pub/sub:
#
#   mailrs-receiver --PUBLISH--> kevy --SUBSCRIBE--> mailrs-webapi --WS--> browser
#
# Nothing in the unit suite can see that whole path, and its failure
# mode is silence: the page connects, reports "connected", and shows an
# empty list, which is what an idle mail server looks like too.
#
# So this opens a real SMTP session against the receiver and reads the
# frames off the channels while it runs.
#
# A NOTE ON THE INSTRUMENT. `kevy-cli SUBSCRIBE` was tried first and
# printed the subscribe confirmation and then nothing — for the trace
# channel AND for the stats channel that webapi demonstrably receives.
# A probe that reports silence on a channel known to be busy is a broken
# probe, and it is indistinguishable from a broken feature. Hence raw
# RESP over a socket here, and hence the control below: the stats
# channel has a 10 s heartbeat, so a run that sees no stats frame is
# reporting on itself, not on the receiver.
#
#   ./scripts/probe-smtp-monitor.sh              # against t02
#   PROD=root@host ./scripts/probe-smtp-monitor.sh
set -euo pipefail
PROD="${PROD:-root@t02.golia.jp}"
WINDOW="${WINDOW:-22}"

ssh "$PROD" 'bash -s' <<EOS
set -u
KEVY_IP=\$(docker inspect mailrs-kevy --format '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' | awk '{print \$1}')
[ -n "\$KEVY_IP" ] || { echo "!! cannot find mailrs-kevy"; exit 1; }

cat > /tmp/mailrs-monitor-probe.py <<'PY'
import socket, sys, time
host = sys.argv[1]
window = float(sys.argv[2])
s = socket.create_connection((host, 6379), timeout=5)
s.sendall(b"*3\r\n\$9\r\nSUBSCRIBE\r\n\$10\r\ntrace:smtp\r\n\$16\r\ntrace:smtp-stats\r\n")
s.settimeout(1.0)
end = time.time() + window
with open("/tmp/mailrs-monitor-probe.out", "wb", buffering=0) as out:
    while time.time() < end:
        try:
            d = s.recv(65536)
        except socket.timeout:
            continue
        if not d:
            break
        out.write(d)
PY

rm -f /tmp/mailrs-monitor-probe.out
nohup python3 /tmp/mailrs-monitor-probe.py "\$KEVY_IP" $WINDOW >/tmp/mailrs-monitor-probe.log 2>&1 &
sleep 3
printf 'EHLO monitor-probe.local\r\nQUIT\r\n' | timeout 8 nc 127.0.0.1 25 >/dev/null || true
sleep $((WINDOW - 5))

echo "--- session trace (trace:smtp) ---"
grep -ao '"type":"[A-Za-z]*"' /tmp/mailrs-monitor-probe.out | sort | uniq -c
echo "--- control: stats heartbeat (10 s), 0 here means the PROBE is broken ---"
grep -ac 'uptime_secs' /tmp/mailrs-monitor-probe.out || true
rm -f /tmp/mailrs-monitor-probe.py /tmp/mailrs-monitor-probe.out /tmp/mailrs-monitor-probe.log
EOS
