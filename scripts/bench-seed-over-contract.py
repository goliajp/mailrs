#!/usr/bin/env python3
"""Seed any core with the benchmark dataset, through the core-api contract.

The kevy arm is seeded by `mailrs-fastcore-migrate`, which reads this same
NDJSON and writes kevy directly. The SQL arm was seeded by handing 98 MB of
batched INSERTs to `spg import`, which after forty minutes was at 99.8% CPU and
3.85 GB resident with nothing on disk — see
`.claude/notes/spg-7.37.16-reactivation-feedback-2026-08-13.md` §3b.

So the SQL arm is seeded here instead, over HTTP, through `deliver_message` —
which both cores serve, and which the migration tool itself uses. That makes the
two arms' seeding *identical* rather than merely equivalent, which is a better
place to be than where the bulk-SQL path left it.

It is not part of what gets timed. The panel starts after the fingerprint check,
and the fingerprint is what proves the arms hold the same rows regardless of how
they got there.

Usage:
    bench-seed-over-contract.py --base http://127.0.0.1:3300 \\
        --secret <shared> < seed.ndjson
"""

from __future__ import annotations

import argparse
import http.client
import json
import sys
import time
from urllib.parse import urlparse


def connect(base: str) -> tuple[http.client.HTTPConnection, str]:
    u = urlparse(base)
    if u.scheme != "http":
        sys.exit(f"only http is supported here, got {u.scheme!r}")
    return http.client.HTTPConnection(u.hostname, u.port or 80, timeout=30), ""


class Core:
    """One keep-alive connection, reconnecting when the server drops it.

    24,000 requests over fresh connections is 24,000 handshakes, and the point
    of this script is to be faster than the thing it replaces.
    """

    def __init__(self, base: str, secret: str) -> None:
        self.base = base
        self.secret = secret
        self.conn, _ = connect(base)

    def post(self, path: str, body: dict) -> int:
        payload = json.dumps(body)
        headers = {"Content-Type": "application/json"}
        if self.secret:
            headers["Authorization"] = f"Bearer {self.secret}"
        for attempt in (1, 2):
            try:
                self.conn.request("POST", path, payload, headers)
                resp = self.conn.getresponse()
                resp.read()
                return resp.status
            except (http.client.HTTPException, OSError):
                # The server closed an idle connection, or we raced its
                # keep-alive timeout. One reconnect, then let it fail loudly:
                # a seeder that silently drops rows produces an arm that looks
                # fast because it holds less.
                if attempt == 2:
                    raise
                self.conn.close()
                self.conn, _ = connect(self.base)
        raise AssertionError("unreachable")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True, help="core-api base, e.g. http://127.0.0.1:3300")
    ap.add_argument("--secret", default="", help="shared bearer secret")
    ap.add_argument("--progress", type=int, default=5000, help="log every N messages")
    args = ap.parse_args()

    core = Core(args.base, args.secret)
    accounts = messages = 0
    failures: list[tuple[str, int]] = []
    started = time.time()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        rec = json.loads(line)
        kind = rec.get("kind")

        if kind == "account":
            b = rec["blob"]
            status = core.post(
                "/v1/admin/accounts",
                {
                    "address": b["address"],
                    "display_name": b.get("display_name", ""),
                    "password": "bench-placeholder-never-used",
                },
            )
            # 409 is the account already being there, which is not a failure.
            if status not in (200, 201, 204, 409):
                failures.append((f"add_account {b['address']}", status))
            accounts += 1

        elif kind == "message":
            w = rec["wire"]
            user, tid = rec["user"], rec["thread_id"]
            status = core.post(
                f"/v1/users/{user}/threads/{tid}/messages",
                {
                    "message_id": rec["message_id"],
                    "subject": w.get("subject", ""),
                    "senders_csv": w.get("sender", ""),
                    "latest_date": rec["internal_date"],
                    "latest_preview": "",
                    "category": rec.get("category", "inbox"),
                    "unread": rec.get("unread", True),
                    "uid": w.get("uid", 0),
                    "payload_wire_json": json.dumps(w),
                },
            )
            if status not in (200, 201, 204):
                failures.append((f"deliver {rec['message_id']}", status))
            messages += 1
            if args.progress and messages % args.progress == 0:
                rate = messages / max(time.time() - started, 0.001)
                print(
                    f"  seeded {messages} messages ({rate:.0f}/s)",
                    file=sys.stderr,
                    flush=True,
                )

    elapsed = time.time() - started
    print(
        f"seeded over the contract: accounts={accounts} messages={messages} "
        f"in {elapsed:.1f}s ({messages / max(elapsed, 0.001):.0f}/s)",
        file=sys.stderr,
    )

    if failures:
        # Loudly, and with the first few, because a partially-seeded arm is the
        # one failure mode that looks like a fast arm rather than a broken one.
        print(f"!! {len(failures)} request(s) failed:", file=sys.stderr)
        for what, status in failures[:10]:
            print(f"     {what} -> {status}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
