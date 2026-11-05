#!/usr/bin/env bash
# archive-parse-smoke.sh — malformed RAR and 7z attachments do not take a
# release build down, or, if one does, it is set aside and counted.
#
# Only a release binary shows this: release builds abort on panic, tests
# unwind. Two readers of attachments are run on the same set of messages:
#
#   1. mailrs-fraud-check over a maildir. It calls the function the
#      receiver calls at SMTP time (mailrs_inbound::body::attachments_inside).
#   2. mailrs-fastcore with a fraud rule file, so the backfill judges every
#      conversation at start (fraud_rescan, the reading rejudge and the
#      maintenance sweeps use). It is restarted as a supervisor would, up
#      to five runs; a run that dies must end in the backfill's skip list
#      and in /v1/healthz.
#
# The archives are synthesized headers: truncations of a valid RAR 4 and
# RAR 5 at every byte, header encryption, declared sizes past the end,
# zero-sized headers, overlong integers, a 20,000-member listing, a 7z
# signature with nothing after it. No network kevy, no docker.
#
# Usage:
#   ./scripts/archive-parse-smoke.sh

set -euo pipefail
cd "$(dirname "$0")/.."

TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/mailrs-archive-smoke-$$"
PORT="${FASTCORE_PORT:-13303}"

cleanup() {
    if [[ -n "${SVR_PID:-}" ]]; then
        kill "$SVR_PID" 2>/dev/null || true
        wait "$SVR_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

if [[ "${SKIP_BUILD:-}" != 1 ]]; then
    cargo build --release -p mailrs-fastcore --bin mailrs-fastcore 2>&1 | tail -3
    cargo build --release -p mailrs-fraud-lua --bin mailrs-fraud-check 2>&1 | tail -3
fi

MAILDIR="$WORK/maildir"
mkdir -p "$WORK/kevy" "$WORK/check/cur" "$WORK/held" "$MAILDIR/local.test/u/"{new,cur,tmp} \
    "$MAILDIR/.spool/incoming/"{new,cur,tmp}

python3 -I - "$WORK" <<'PYEOF'
import base64, json, os, sys

work = sys.argv[1]


def vint(v):
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v == 0:
            out.append(b)
            return bytes(out)
        out.append(b | 0x80)


def blk5(body, data=b""):
    return b"\0\0\0\0" + vint(len(body)) + body + data


def file5(name, extra=b""):
    flags = 0x02 | (0x01 if extra else 0)
    body = vint(2) + vint(flags) + (vint(len(extra)) if extra else b"") + vint(0)
    body += vint(0) + vint(0) + vint(0x20) + vint(0) + vint(0)
    body += vint(len(name)) + name + extra
    return blk5(body)


RAR5 = b"Rar!\x1a\x07\x01\x00"
RAR4 = b"Rar!\x1a\x07\x00"


def blk4(kind, flags, rest):
    return b"\0\0" + bytes([kind]) + flags.to_bytes(2, "little") + (7 + len(rest)).to_bytes(2, "little") + rest


def file4(name, flags=0, packed=0):
    rest = packed.to_bytes(4, "little") + bytes(4) + b"\x02" + bytes(8) + b"\x1d\x30"
    rest += len(name).to_bytes(2, "little") + (0x20).to_bytes(4, "little") + name
    return blk4(0x74, 0x8000 | flags, rest)


good5 = RAR5 + blk5(b"\x01\x00\x00") + file5(b"order.bat") + file5(b"a.pdf", b"\x02\x01\x00") + blk5(b"\x05\x00\x00")
good4 = RAR4 + blk4(0x73, 0, bytes(6)) + file4(b"docs\\run.exe") + file4(b"a.pdf\0\0\x00a.ex\x00e", 0x0200) + blk4(0x7B, 0x4000, b"")

cases = {}
for i in range(len(good5) + 1):
    cases[f"rar5-cut-{i:03}"] = good5[:i]
for i in range(len(good4) + 1):
    cases[f"rar4-cut-{i:03}"] = good4[:i]
cases["rar5-header-encrypted"] = RAR5 + blk5(b"\x04\x00\x00\x00") + b"\xee" * 64
cases["rar4-header-encrypted"] = RAR4 + blk4(0x73, 0x0080, bytes(6)) + b"\xee" * 64
cases["rar5-huge-header"] = RAR5 + b"\0\0\0\0" + vint(1 << 62) + b"\x01\x00\x00"
cases["rar5-huge-data"] = RAR5 + blk5(vint(2) + vint(2) + vint(1 << 62)) + b"x" * 32
cases["rar5-huge-name"] = RAR5 + blk5(b"\x02\x00\x00\x00\x20\x00\x00" + vint(1 << 40) + b"a.exe")
cases["rar5-huge-extra"] = RAR5 + blk5(b"\x02\x01" + vint(1 << 50) + b"\x00")
cases["rar5-zero-header"] = RAR5 + b"\0" * 64
cases["rar5-overlong-vint"] = RAR5 + b"\0\0\0\0" + b"\xff" * 16
cases["rar5-many"] = RAR5 + file5(b"a.txt") * 20000 + blk5(b"\x05\x00\x00")
cases["rar4-huge-pack"] = RAR4 + file4(b"a.exe", 0x0100, 0xFFFFFFFF) + b"\xff" * 8
cases["rar4-zero-size"] = RAR4 + b"\0\0\x74\0\x80\0\0" * 8
cases["rar4-bad-unicode"] = RAR4 + file4(b"x\0" + b"\xff" * 200, 0x0200) + blk4(0x7B, 0, b"")
cases["7z-signature-only"] = b"7z\xbc\xaf\x27\x1c"
cases["7z-garbage"] = b"7z\xbc\xaf\x27\x1c\x00\x04" + b"\xff" * 64

env = json.dumps({"reverse_path": "sender@outside.example", "forward_paths": ["u@local.test"],
                  "is_authenticated": False, "conn_id": 1, "target_folder": "INBOX",
                  "received_at": 1790000000, "schema_version": 1}, separators=(",", ":"))
seeds = os.path.join(work, "seeds")
os.makedirs(seeds)
for n, (name, archive) in enumerate(sorted(cases.items())):
    with open(os.path.join(seeds, name), "wb") as f:
        f.write(archive)
    att = base64.encodebytes(archive).decode().replace("\n", "\r\n")
    msg = (f"From: Orders <orders@outside.example>\r\nTo: u@local.test\r\n"
           f"Subject: case {n}\r\nMessage-ID: <case-{n}@outside.example>\r\n"
           f"Date: Tue, 15 Sep 2026 09:00:00 +0000\r\nMIME-Version: 1.0\r\n"
           f"Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n"
           f"Content-Type: text/plain\r\n\r\nsee attached\r\n--b\r\n"
           f"Content-Type: application/octet-stream\r\n"
           f"Content-Disposition: attachment; filename=\"{name}.rar\"\r\n"
           f"Content-Transfer-Encoding: base64\r\n\r\n{att}--b--\r\n").encode()
    with open(os.path.join(work, "check", "cur", f"{n:04}"), "wb") as f:
        f.write(msg)
    spool = b"X-Mailrs-Spool-Envelope: " + base64.b64encode(env.encode()) + b"\r\n" + msg
    with open(os.path.join(work, "held", f"1790000000.M{n}P1.case"), "wb") as f:
        f.write(spool)
print(len(cases), file=open(os.path.join(work, "count"), "w"))
PYEOF
COUNT=$(cat "$WORK/count")
echo "messages: $COUNT"
if [[ -n "${KEEP_SEEDS:-}" ]]; then
    mkdir -p "$KEEP_SEEDS" && cp "$WORK/seeds/"* "$KEEP_SEEDS/"
fi

fail=0
check() {
    if eval "$2"; then
        printf "  PASS %s\n" "$1"
    else
        printf "  FAIL %s\n" "$1"
        fail=1
    fi
}

PACK="$WORK/rules.lua"
cat crates/fraud-lua/rules/{brands,helpers,identity,provenance,content}.lua > "$PACK"

code=0
./target/release/mailrs-fraud-check compare builtin "$PACK" "$WORK/check" "$COUNT" \
    > "$WORK/check.json" 2> "$WORK/check.err" || code=$?
checked=$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["checked"])' "$WORK/check.json" 2>/dev/null || echo none)
echo "fraud-check: exit $code, checked $checked of $COUNT"
check "fraud-check read every message and exited 0" '[[ $code -eq 0 && "$checked" == "$COUNT" ]]'

SECRET=$(openssl rand -hex 16)
# the secret stays out of every argv: the header is read from a file
(umask 077 && printf 'Authorization: Bearer %s\n' "$SECRET" > "$WORK/auth-header")
# $1: the fraud rule file, or empty for none
start() {
    if [[ -n "$1" ]]; then
        export MAILRS_FRAUD_RULES_FILE="$1"
    else
        unset MAILRS_FRAUD_RULES_FILE
    fi
    MAILRS_CORE_API_SECRET="$SECRET" MAILRS_KEVY_DATA_DIR="$WORK/kevy" MAILRS_MAILDIR="$MAILDIR" \
        MAILRS_FASTCORE_BIND="127.0.0.1:$PORT" MAILRS_IMAP_BIND=off MAILRS_IMAPS_BIND=off \
        MAILRS_POP3_BIND=off MAILRS_POP3S_BIND=off MAILRS_MANAGESIEVE_BIND=off \
        ./target/release/mailrs-fastcore >>"$WORK/fastcore.log" 2>&1 &
    SVR_PID=$!
}

# Setup run, no rule file: make the account, hand the spool the messages,
# wait (at most 120 s) until all are delivered, stop.
start ""
for _ in $(seq 1 40); do
    curl -s -o /dev/null "http://127.0.0.1:$PORT/v1/healthz" && break
    sleep 0.5
done
curl -s -o /dev/null -w "account: %{http_code}\n" -X POST -H @"$WORK/auth-header" \
    -H 'content-type: application/json' "http://127.0.0.1:$PORT/v1/admin/accounts" \
    -d '{"address":"u@local.test","display_name":"u","password":"","password_hash":""}'
mv "$WORK/held/"* "$MAILDIR/.spool/incoming/new/"
count_delivered() {
    find "$MAILDIR/local.test/u/new" "$MAILDIR/local.test/u/cur" -type f | wc -l | tr -d ' '
}
for _ in $(seq 1 240); do
    [[ "$(count_delivered)" == "$COUNT" ]] && break
    sleep 0.5
done
kill "$SVR_PID"
wait "$SVR_PID" || true
SVR_PID=

# The backfill starts 15 s after boot. A run counts as done when the
# backfill's progress says complete, or the process dies; at most 120 s.
progress="$WORK/kevy/fraud-backfill/progress.json"
deaths=0
for run in 1 2 3 4 5; do
    start "$PACK"
    for _ in $(seq 1 240); do
        kill -0 "$SVR_PID" 2>/dev/null || break
        grep -q '"complete":true' "$progress" 2>/dev/null && break
        sleep 0.5
    done
    if kill -0 "$SVR_PID" 2>/dev/null; then
        echo "run $run: fastcore is up"
        break
    fi
    code=0
    wait "$SVR_PID" || code=$?
    SVR_PID=
    deaths=$((deaths + 1))
    echo "run $run: fastcore exited with $code"
done

health=$(curl -s "http://127.0.0.1:$PORT/v1/healthz" || true)
echo "healthz: $health"
echo "progress: $(cat "$progress" 2>/dev/null || echo none)"
delivered=$(count_delivered)
echo "delivered: $delivered of $COUNT; deaths: $deaths"
check "every message was delivered" '[[ "$delivered" == "$COUNT" ]]'
check "fastcore is up" '[[ -n "${SVR_PID:-}" ]]'
check "the backfill judged every conversation" 'grep -q "\"complete\":true" "$progress" && grep -q "\"total\":$COUNT" "$progress"'
if (( deaths == 0 )); then
    check "no run died" 'true'
    check "health counts nothing skipped" '[[ "$health" == *"\"fraud_backfill\":0"* ]]'
    check "the spool quarantined nothing" '[[ "$health" == *"\"quarantined\":0"* ]]'
else
    check "every death is in the skip list" '[[ "$health" == *"\"fraud_backfill\":"* && "$health" != *"\"fraud_backfill\":0"* ]]'
fi
check "nothing panicked" '! grep -q "panicked" "$WORK/fastcore.log" "$WORK/check.err"'
if (( fail )); then
    tail -40 "$WORK/fastcore.log"
    exit 1
fi
echo "archive parse smoke passed"
