#!/usr/bin/env python3
"""Emit the interaction-latency dataset: NDJSON for mailrs-fastcore-migrate,
plus one maildir file per message so the reading pane has real bodies.

One user with THREADS conversations (default 30,000), about the size of
the largest real mailbox. Every fifth conversation has a reply, so the
reading pane opens one or two messages. Bodies are about 2 KB of text;
every fourth message is multipart/alternative with an HTML part, so the
HTML rendering path is exercised as well.

Markers the latency spec relies on:

  - the subject of conversation N ends with "#N"
  - every body of conversation N contains "REF-N"

so the spec can tell, from the DOM alone, that the reading pane shows the
conversation the list has selected and not the previous one.

Unread: the newest UNREAD_HEAD conversations alternate two unread, one read
(the mark-read interaction needs thirty unread rows near the top); older
ones are unread one time in five.

Usage:
  latency-seed.py --maildir DIR [--threads N] > seed.ndjson
"""

import argparse
import json
import os
import random
import sys
from email.utils import formatdate

DOMAIN = "latency.local"
LOCAL = "bench"
ADDRESS = f"{LOCAL}@{DOMAIN}"
# argon2id of "bench-password", fixed salt — the same hash bench-api-seed.py uses
HASH = "$argon2id$v=19$m=19456,t=2,p=1$YmVuY2hzYWx0YmVuY2hzYWx0$aTmjX/qlKnp01PFBFtErHl76zMHz65r0mifk1CAgQZQ"

BASE_EPOCH = 1_760_000_000
UNREAD_HEAD = 300

# 200 search terms. Each subject draws five, so one term matches roughly
# 2.5% of the conversations — about 750 of 30,000, enough for a full first
# screen of results.
TERMS = [f"{a}{b}" for a in (
    "alpha", "bravo", "cedar", "delta", "ember", "fjord", "garnet", "harbor",
    "indigo", "juniper", "kestrel", "lumen", "marble", "nimbus", "onyx",
    "prism", "quartz", "raven", "sierra", "tundra",
) for b in ("", "x", "line", "port", "field", "stone", "wave", "gate", "mark", "core")]

FILLER = (
    "please find the details below and let me know if anything needs to "
    "change before we send the final version to the team next week the "
    "numbers were checked against last quarter and the schedule still holds"
).split()

SENDERS = [f"Person {i} <person{i}@example{i % 9}.com>" for i in range(120)]


def body_text(rng: random.Random, n: int) -> str:
    lines = []
    words = [rng.choice(FILLER) for _ in range(300)]
    for i in range(0, len(words), 15):
        lines.append(" ".join(words[i : i + 15]))
    lines.insert(2, f"Reference REF-{n} for this conversation.")
    return "\r\n".join(lines)


def raw_message(rng, n, uid, sender, subject, epoch, msgid, in_reply_to, html):
    text = body_text(rng, n)
    head = [
        f"From: {sender}",
        f"To: Bench <{ADDRESS}>",
        f"Subject: {subject}",
        f"Message-ID: <{msgid}>",
        f"Date: {formatdate(epoch)}",
        "MIME-Version: 1.0",
    ]
    if in_reply_to:
        head.append(f"In-Reply-To: <{in_reply_to}>")
        head.append(f"References: <{in_reply_to}>")
    if not html:
        head.append("Content-Type: text/plain; charset=utf-8")
        return ("\r\n".join(head) + "\r\n\r\n" + text + "\r\n").encode()
    boundary = f"b{uid}"
    html_body = "<html><body>" + "".join(
        f"<p>{line}</p>" for line in text.split("\r\n")
    ) + "</body></html>"
    head.append(f'Content-Type: multipart/alternative; boundary="{boundary}"')
    parts = [
        f"--{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n",
        f"--{boundary}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{html_body}\r\n",
        f"--{boundary}--\r\n",
    ]
    return ("\r\n".join(head) + "\r\n\r\n" + "".join(parts)).encode()


def unread_of(rng, n, threads):
    if n > threads - UNREAD_HEAD:
        return n % 3 != 0
    return rng.random() < 0.2


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--maildir", required=True, help="MAILRS_MAILDIR root to write into")
    ap.add_argument("--threads", type=int, default=30_000)
    args = ap.parse_args()

    rng = random.Random(2026_11_03)
    box = os.path.join(args.maildir, DOMAIN, LOCAL)
    for sub in ("cur", "new", "tmp"):
        os.makedirs(os.path.join(box, sub), exist_ok=True)

    out = sys.stdout
    account = {
        "address": ADDRESS,
        "domain": DOMAIN,
        "display_name": "Bench User",
        "active": True,
        "created_at": BASE_EPOCH,
        "quota_bytes": 0,
        "password_hash": HASH,
    }
    out.write(json.dumps({"kind": "account", "blob": account}, separators=(",", ":")) + "\n")

    uid = 0
    messages = 0
    for n in range(1, args.threads + 1):
        # A thread is keyed by its root Message-ID, as ingest keys it. Any
        # other id makes fastcore's maildir self-heal find files with no
        # thread under their root and create a second row for each.
        tid = f"m{n}.0@{DOMAIN}"
        subject = " ".join(rng.choice(TERMS) for _ in range(5)) + f" #{n}"
        category = rng.choices(("inbox", "notification", "promotion"), (8, 1, 1))[0]
        unread = unread_of(rng, n, args.threads)
        epoch = BASE_EPOCH + n * 300
        sender = rng.choice(SENDERS)
        chain = [(sender, subject, epoch)]
        if n % 5 == 0:
            chain.append((rng.choice(SENDERS), "Re: " + subject, epoch + 120))
        prev = ""
        for i, (frm, subj, at) in enumerate(chain):
            uid += 1
            msgid = f"m{n}.{i}@{DOMAIN}"
            blob = f"{at}.M{uid}.seed"
            last = i == len(chain) - 1
            seen = not (unread and last)
            raw = raw_message(rng, n, uid, frm, subj, at, msgid, prev, uid % 4 == 0)
            if seen:
                path = os.path.join(box, "cur", f"{blob}:2,S")
            else:
                path = os.path.join(box, "new", blob)
            with open(path, "wb") as f:
                f.write(raw)
            wire = {
                "id": 0,
                "mailbox_id": 0,
                "uid": uid,
                "blob_ref": blob,
                "sender": frm,
                "recipients": ADDRESS,
                "subject": subj,
                "date": at,
                "internal_date": at,
                "size": len(raw),
                "flags": 1 if seen else 0,
                "message_id": msgid,
                "in_reply_to": prev,
                "sender_trust": "",
                "thread_id": tid,
                "modseq": uid,
                "user_address": ADDRESS,
            }
            rec = {
                "kind": "message",
                "user": ADDRESS,
                "thread_id": tid,
                "message_id": msgid,
                "internal_date": at,
                "category": category,
                "unread": not seen,
                "wire": wire,
            }
            out.write(json.dumps(rec, separators=(",", ":")) + "\n")
            prev = msgid
            messages += 1

    print(
        f"seed: threads={args.threads} messages={messages} address={ADDRESS} maildir={box}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
