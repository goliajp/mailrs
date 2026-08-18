"""A mailrs REST API, reduced to what the iOS UI tests drive.

Serves the shapes the Rust handlers send, not shapes convenient for the
Swift models — `/api/conversations` is a bare array because
`conversations.rs` returns `Json<Vec<ConversationResponse>>`, and a stub
that wrapped it in an envelope would let a wrong client model pass.

Fixtures only. It never reaches a real mailbox and holds no credentials:
any password logs in, because what the tests are about is what the app
does afterwards.

Port 6039, registered to this project in the shared port registry.
`scripts/ios-build.sh` starts and stops it around the UI tests — they
failed as "inbox never listed" the first time it was not running, which
is a confusing way to be told a dependency is missing.
"""

import json
import os
import re
import base64
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, unquote, urlparse

WIDE = ('<table width="760" style="width:760px"><tr><td>'
        '<div style="width:760px;background:#eef;padding:8px">'
        # A remote image, as a newsletter carries: the client must not
        # fetch it until asked, because fetching is what tells the
        # sender the message was opened.
        '<img src="https://tracker.example.com/open.gif" width="1" height="1">'
        '<h1>Newsletter</h1><p>' + ('lorem ipsum dolor sit amet ' * 12) + '</p>'
        '</div></td></tr></table>')

def _paged_convos(limit, before_ts):
    """`/api/conversations` the way `list_threads/paths.rs` answers it.

    Keyset, `latest_date < before_ts`, newest first, no `has_more`. The
    comparison is strict — the handler passes `Some(ts - 1)` as an
    inclusive bound — which is the whole reason the client asks for one
    second past its oldest row.

    Rows 48-52 deliberately share a second. A client that paged on its
    oldest row's own timestamp would skip the ones that did not fit, and
    a stub without a collision could never show that.
    """
    rows = []
    for i in range(120):
        ts = BASE_TS - i * 3600
        if 48 <= i <= 52:
            ts = BASE_TS - 48 * 3600
        rows.append({
            "thread_id": f"p{i}", "subject": f"Paged thread {i}",
            "participants": [f"sender{i}@example.com"], "message_count": 1,
            "unread_count": 0, "last_date": ts, "category": "inbox",
            "flagged": False, "snippet": f"body {i}", "pinned": False,
            "archived": False, "importance_level": "normal",
            "importance_score": 0.0, "requires_action": False,
            "received_count": 1, "sent_count": 0,
        })
    rows.sort(key=lambda r: -r["last_date"])
    if before_ts is not None:
        rows = [r for r in rows if r["last_date"] < before_ts]
    return rows[:limit]


BASE_TS = 1754400000


def convo(tid, subject, snippet, ts):
    return {
        "thread_id": tid, "subject": subject, "participants": ["someone@example.com"],
        "message_count": 1, "unread_count": 0, "last_date": ts, "category": "inbox",
        "flagged": False, "snippet": snippet, "pinned": False, "archived": False,
        "importance_level": "normal", "importance_score": 0.0, "requires_action": False,
        "received_count": 1, "sent_count": 0,
    }

CONVOS = [{
    "thread_id": "t1", "subject": "Quarterly report and the follow-up notes",
    "participants": ["Alice Smith <alice@example.com>"],
    "message_count": 2, "unread_count": 2,
    "last_date": 1754400000, "category": "inbox", "flagged": False,
    "snippet": "Please review before Friday, ref 2026", "pinned": False, "archived": False,
    "importance_level": "normal", "importance_score": 0.5, "requires_action": False,
    "received_count": 2, "sent_count": 0,
}, {
    "thread_id": "t2", "subject": "請求書のご送付につきまして",
    "participants": ["keiri@example.co.jp"], "message_count": 1, "unread_count": 1,
    "last_date": 1754300000, "category": "inbox", "flagged": False,
    "snippet": "ご確認ください。ref 2026", "pinned": False, "archived": False,
    "importance_level": "normal", "importance_score": 0.2, "requires_action": False,
    "received_count": 1, "sent_count": 0,
}]

# A one-pixel PNG, so the attachment path carries real bytes with a real
# content type rather than a text file pretending.
PIXEL_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)

# Which attachment indices have been fetched, in order, exposed at
# `/debug/fetched`. The UI test asserts on this because it is the only
# thing that can tell the indices apart: both files preview the same, so
# a client that always asked for index 0 would open a preview and look
# correct. That is exactly what an earlier version of the test did.
FETCHED = []

# Bodies POSTed to /api/mail/send, exposed at `/debug/sent`. A new
# message and a reply differ only in two fields nothing on screen shows,
# so the test reads them here rather than guessing from the UI.
# Aliases, in the `{items: [...]}` envelope the admin endpoint uses —
# deliberately unlike /api/conversations, which is a bare array.
ALIASES = [
    # Targets the signed-in address, so mail sent to it arrived *via an
    # alias* — the case the thread has to mark. The second targets
    # somebody else and must never be marked as mine.
    {"id": 1, "source_address": "sales@golia.jp", "target_address": "me@golia.jp",
     "domain": "golia.jp", "alias_type": "alias", "active": True, "created_at": 1754400000},
    {"id": 2, "source_address": "info@golia.ai", "target_address": "lihao@golia.jp",
     "domain": "golia.ai", "alias_type": "alias", "active": False, "created_at": 1754400001},
]
ALIAS_COUNTER = [2]

# The queue as list_admin_queue answers it: the job blob plus the list
# it was found in. Everything but the identity is optional, and one row
# deliberately has neither error nor attempts — a healthy job is the
# shape a client is most likely to get wrong.
# Built per request, not once at import. A fixed epoch in 2025 is
# permanently in the past, so a client that only renders *future*
# retries — the only kind worth printing — would show nothing and the
# test would pass on an empty row. Freezing the offsets at start-up
# fixes that but sets a timer: fifteen minutes into a long suite the
# "future" retry is in the past again, and the assertion fails for
# reasons that have nothing to do with the code.
def queue_jobs():
    now = int(time.time())
    return [
        {"id": 7, "sender": "lihao@golia.jp", "recipient": "stuck@example.com",
         "status": "pending", "attempts": 3, "last_error": "421 too many connections",
         "next_retry": now + 900, "scheduled_at": None, "created_at": now - 7200},
        {"id": 8, "sender": "lihao@golia.jp", "recipient": "fresh@example.com",
         "status": "inflight", "created_at": now - 60},
        # Asked for later, not stuck. Before the queue row read its own
        # timestamps this was indistinguishable from the row above it.
        {"id": 9, "sender": "lihao@golia.jp", "recipient": "later@example.com",
         "status": "pending", "scheduled_at": now + 86400, "created_at": now - 120},
    ]
SUPPRESSED = ["bounced@example.com", "closed@example.com"]

# Per-account side state. The sieve script is one string under "script";
# `quota_bytes` is null for an account with no cap, which is a different
# answer from zero and the reason it is nullable on the wire.
SIEVE = {"lihao@golia.jp": "require [\"fileinto\"];\nif header :contains \"subject\" \"[ops]\" {\n  fileinto \"Ops\";\n}"}
WEBHOOKS = {"lihao@golia.jp": [
    {"id": 5, "account_address": "lihao@golia.jp", "url": "https://hooks.example/mail",
     "event_type": "message.received", "signing_secret": "whsec_x", "active": True,
     "created_at": 1754400000},
]}
# Applications holding credentials against this server.
APPS = [
    {"id": 1, "app_id": "app_reporting", "name": "Reporting", "description": "",
     "owner_address": "lihao@golia.jp", "scopes": ["mail.read"], "active": True,
     "created_at": 1754400000},
]

# Per-user sender allow/block, as `spam_lists.rs` answers them:
# `{"entries": [...]}`, not `{"items": [...]}` like the admin lists.
# A client that reached for the wrong key would decode an empty list
# and show an empty screen, which looks like "nothing is listed".
SPAM_LISTS = {"whitelist": ["friend@example.com"], "blacklist": ["spammer@example.com"]}

# Agent keys. The stored record has no secret in it — the server keeps
# eight characters — so a client that expected one would decode nothing.
AGENT_KEYS = [
    {"id": 1, "name": "Scheduler", "scopes": ["mail.send"],
     "prefix": "mk_a1b2c", "created_at": 1754400000},
]
AGENT_KEY_COUNTER = [1]

# Anything that reaches here came from inside a rendered message body,
# which is the one thing a mail client must never let happen. Four
# messages in a 900-message sample of the real mailbox carry a <form>.
PHISH_HITS = [0]

# Permission groups. One builtin (no domain, undeletable) and one
# ordinary — the builtin is the shape a client is most likely to get
# wrong, by offering a delete the server will refuse.
PERM_GROUPS = [
    {"id": 1, "name": "Administrators", "description": "", "is_builtin": True,
     "created_at": 1754400000},
    {"id": 2, "name": "Support", "domain": "golia.jp", "description": "",
     "is_builtin": False, "created_at": 1754400001},
]
PERM_CATALOGUE = [
    "mail.send", "mail.read", "mail.read_domain",
    "admin.domains", "admin.accounts", "admin.aliases",
    "admin.groups", "admin.queue", "admin.sieve",
    "admin.impersonate", "internal.rpc",
]
GROUP_GRANTS = {1: ["admin.accounts", "admin.aliases"], 2: ["mail.read"]}
GROUP_PEOPLE = {1: ["lihao@golia.jp"], 2: []}

# The audit log, with a bare action among the dotted ones — the server
# is free to write one, and a client that assumed a dot would show an
# empty verb.
AUDIT = [
    {"id": 3, "timestamp": 1754400200, "actor": "me@golia.jp",
     "action": "alias.delete", "target": "old@golia.jp", "detail": "-> lihao@golia.jp"},
    {"id": 2, "timestamp": 1754400100, "actor": "me@golia.jp",
     "action": "alias.create", "target": "sales@golia.jp", "detail": "-> lihao@golia.jp"},
    {"id": 1, "timestamp": 1754400000, "actor": "me@golia.jp",
     "action": "login", "target": "me@golia.jp", "detail": ""},
]

# DMARC as the handlers answer it. The rollup carries the window's own
# totals rather than leaving the client to add up the rows — and one
# source deliberately loses mail, because a screen where everything
# passes cannot show whether it would surface the one that does not.
DMARC_REPORTS = [
    {"sid": "google.com!abc", "org_name": "google.com", "email": "noreply-dmarc@google.com",
     "policy_domain": "golia.jp", "begin": 1754352000, "end": 1754438400,
     "p": "quarantine", "total": 120, "passing": 118, "rows": 3},
    {"sid": "yahoo.com!def", "org_name": "yahoo.com", "email": "dmarc@yahoo.com",
     "policy_domain": "golia.jp", "begin": 1754265600, "end": 1754352000,
     "p": "none", "total": 40, "passing": 40, "rows": 1},
]
DMARC_SOURCES = {
    "items": [
        {"source_ip": "203.0.113.10", "total": 150, "passing": 150, "domains": ["golia.jp"]},
        {"source_ip": "198.51.100.7", "total": 10, "passing": 8, "domains": ["golia.jp"]},
    ],
    "total": 160, "passing": 158, "reports": 2,
}

# Groups list under `items` like the other admin collections, but their
# members come back under `members` as bare addresses — a difference the
# client has to hold rather than assume away.
GROUPS = [
    {"id": 1, "address": "team@golia.jp", "domain": "golia.jp", "name": "Team",
     "description": "", "created_at": 1754400000},
]
GROUP_MEMBERS = {1: ["lihao@golia.jp", "Keiri <keiri@golia.jp>"]}
GROUP_COUNTER = [1]

ACCOUNTS = [
    {"address": "lihao@golia.jp", "domain": "golia.jp", "display_name": "Li Hao",
     "active": True, "created_at": 1754400000, "quota_bytes": 5368709120},
    {"address": "noreply@golia.jp", "domain": "golia.jp", "display_name": "",
     "active": False, "created_at": 1754400001, "quota_bytes": 0},
]
DOMAINS = [
    {"name": "golia.jp", "created_at": 1754400000},
    {"name": "golia.ai", "created_at": 1754400001},
]

# What reached POST /api/admin/accounts: the address, and whether a
# password came with it — never the password.
ACCOUNT_POSTS = []

SENT = []

# Every q= the contacts endpoint has answered, for debounce assertions.
CONTACT_QUERIES = []

# Drafts, kept as the server keeps them: upsert on a supplied id,
# allocate one otherwise. Modelling the id behaviour rather than always
# appending is the point — a client that posted without its id would
# leave a trail here, which is exactly the bug to catch.
DRAFTS = {}
DRAFT_COUNTER = [0]

# Every draft POST body, so a client that keeps losing its id shows up as
# a run of `null`s rather than only as a count.
DRAFT_POSTS = []

# Every non-GET path, in order, at `/debug/writes`. The read/star verbs
# answer 204 with no body, so which of them fired — and how many times —
# is invisible on screen; the tests read it here.
WRITES = []

# How many times the unseen count was asked for — the badge's input.
# The badge itself belongs to the OS and no test can read the icon, so
# the client behaviour worth pinning is "refreshed at the moments the
# number may have moved".
UNSEEN_FETCHES = [0]

# How many times the conversation list has been asked for, so a test
# can tell a refresh from a screen that merely came back.
LIST_FETCHES = [0]

# Milliseconds to sit on before answering the conversation list. The
# empty-state test needs the first page to be observably in flight —
# without a delay the stub answers faster than XCUITest can look.
LIST_DELAY_MS = [0]

ATTACHMENTS = [
    {"filename": "請求書_2026年8月分.pdf", "content_type": "application/pdf", "size": 1234},
    {"filename": "logo.png", "content_type": "image/png", "size": len(PIXEL_PNG)},
]


def msg(uid, sender, trust, html):
    # Senders carry display names, as real mail does — the clients must
    # show the name and keep the address for the wire.
    recipients = "me@golia.jp, Bob <bob@example.com>" if uid == 2 else "me@golia.jp"
    return {"uid": uid, "sender": sender, "sender_trust": trust,
            "recipients": recipients, "subject": "Quarterly report", "flags": 0,
            "internal_date": 1754400000, "message_id": f"<m{uid}@x>",
            "text_body": "plain fallback", "html_body": html,
            "attachments": ATTACHMENTS if uid == 1 else [],
            "category": "inbox", "risk_score": 0, "risk_reason": "", "summary": "",
            "people": {}, "dates": {}, "amounts": {}, "action_items": [],
            "ai_analyzed": False, "importance_level": "normal", "importance_score": 0.1,
            "is_bulk_sender": False, "has_tracking_pixel": False,
            "requires_action": False, "sender_intent": ""}

MESSAGES = [msg(1, "Alice Smith <alice@example.com>", "verified", WIDE),
            msg(2, "spoofed@example.com", "suspicious", "<p>Short reply, narrow body.</p>")]

# A newsletter, which is what 42.6% of real mail is. uid 7 accepts RFC
# 8058 one-click; uid 8 only offers a page, so the client must say so
# rather than pretending it can leave the list on the reader's behalf.
# Order matters: a thread opens with its **last** message expanded, so
# the one-click case is last. With it first, the card on screen was the
# page-only one and tapping its button left for Safari — which is the
# correct behaviour for that offer, and useless for testing this one.
NEWSLETTER = [
    dict(msg(7, "Jalan <point-j@jalan.example>", "verified",
             "<p>\u30dd\u30a4\u30f3\u30c8\u306e\u304a\u77e5\u3089\u305b</p>"),
         unsubscribe={"one_click": False,
                      "http": ["https://jalan.example/unsubscribe?id=9"]}),
    dict(msg(8, "ByteByteGo <news@substack.example>", "verified",
             "<p>This week in systems design.</p>"),
         unsubscribe={"one_click": True,
                      "http": ["https://substack.example/u?t=abc"],
                      "mailto": ["mailto:unsub@substack.example"]}),
]

# What the server was asked to unsubscribe from, and whether it agreed.
# `/debug/unsubscribed` is how a test proves the request carried the
# message's identity and not a URL from the client.
# Every thread verb the client has posted, in order: "<verb> <thread>".
VERBS = []
# Verbs the stub should refuse, set by `/debug/refuse-verb`.
VERB_REFUSE = set()
UNSUBSCRIBED = []
UNSUB_REFUSE = [False]
# When set, every authorized request answers 401 — a token that expired
# or an operator who revoked it. A client that only prints the message
# goes on believing it is signed in.
REJECT_SESSION = [False]

# The second thread arrived at an alias, which is its whole purpose
# here: the direct address is absent, so the client has to work out that
# sales@ is one of mine and say so.
# The body is a credential form posting to this stub, plus a meta
# refresh. Neither needs JavaScript, so switching JavaScript off — which
# this client does — stops neither of them.
PHISH_BODY = (
    "<p>\u8acb\u6c42\u66f8\u3092\u304a\u9001\u308a\u3057\u307e\u3059\u3002</p>"
    "<meta http-equiv=\"refresh\" content=\"0; url=http://localhost:6039/debug/phish?via=refresh\">"
    "<form action=\"http://localhost:6039/debug/phish\" method=\"post\">"
    "<input type=\"password\" name=\"p\" value=\"hunter2\">"
    "<input type=\"submit\" value=\"Sign in to continue\">"
    "</form>"
)
# The display name claims a brand; the address is somewhere else
# entirely. Six From headers of this exact shape were in the real
# mailbox when the rule was written.
ALIAS_THREAD = [dict(msg(5, "Amazon.co.jp <no-reply@mail07.jqjintaiyang.example>",
                         "verified", PHISH_BODY),
                     recipients="Sales <sales@golia.jp>",
                     attachments=[{"filename": "smime.p7s",
                                   "content_type": "application/pkcs7-signature",
                                   "size": 2048},
                                  {"filename": "\u8acb\u6c42\u66f8.pdf",
                                   "content_type": "application/pdf", "size": 1234}])]

# Real mail, pointed at rather than committed.
#
# `MAILRS_STUB_REAL=<file.json>` loads messages captured from a live
# mailbox and serves them as extra threads, so the client can be looked
# at rendering what it will actually be given — 600px marketing tables,
# CJK newsletters, `cid:` inline images, a 20KB plain-text digest. The
# file stays out of the repository: fixtures in here are written by
# hand precisely so that nobody's mail has to live in git.
REAL_THREADS = []
REAL_MESSAGES = {}


def _load_real():
    path = os.environ.get("MAILRS_STUB_REAL")
    if not path or not os.path.exists(path):
        return
    with open(path) as fh:
        blob = json.load(fh)
    for i, (key, rec) in enumerate(sorted(blob.items())):
        tid = f"real-{key}"
        # Through `convo`, not by hand: the wire type requires fields a
        # hand-written row forgets, and the client refuses the whole
        # list over one missing `pinned` — which is the correct
        # behaviour and exactly how this was found.
        row = convo(tid, rec.get("subject") or f"({key})", key, 1754400000 - i)
        row["participants"] = [rec.get("sender", "")]
        REAL_THREADS.append(row)
        REAL_MESSAGES[tid] = [{
            "uid": 9000 + i, "sender": rec.get("sender", ""), "sender_trust": "verified",
            "recipients": "me@golia.jp", "subject": rec.get("subject", ""), "flags": 0,
            "internal_date": 1754400000 - i, "message_id": f"<{tid}@x>",
            "text_body": rec.get("text") or "", "html_body": rec.get("html") or "",
            "attachments": [
                {"filename": a.get("filename") or "part",
                 "content_type": a.get("content_type") or "application/octet-stream",
                 "size": a.get("size") or 0, "content_id": a.get("content_id")}
                for a in rec.get("attachments", [])
            ],
            "category": "inbox", "risk_score": 0, "risk_reason": "", "summary": "",
            "people": {}, "dates": {}, "amounts": {}, "action_items": [],
            "ai_analyzed": False, "importance_level": "normal", "importance_score": 0.1,
            "is_bulk_sender": False, "has_tracking_pixel": False,
            "requires_action": False, "sender_intent": "",
        }]


_load_real()


class H(BaseHTTPRequestHandler):
    # **HTTP/1.0 on purpose — do not "upgrade" this to 1.1.** Tried, and
    # it failed 26 of 34 Android tests: several handlers here answer
    # without reading the request body, which a closing connection
    # forgives and a reused one does not — the unread bytes become the
    # next request. The Android emulator's connect stalls that prompted
    # it are fixed where they belong, with `adb reverse` in
    # `scripts/android-build.sh`, so the guest never crosses the NAT.
    def _send(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        # Said out loud rather than left to be inferred from HTTP/1.0.
        # A client that pools the socket and sends a second request into
        # it gets "unexpected end of stream" — the failure this stub
        # spent a suite's worth of flakes producing.
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def _send_bytes(self, body, content_type, filename):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        # RFC 6266, both forms — the same shape `messages.rs` sends.
        self.send_header(
            "Content-Disposition",
            f"attachment; filename=\"attachment\"; filename*=UTF-8''{quote(filename)}",
        )
        self.end_headers()
        self.wfile.write(body)

    def _session_rejected(self):
        """401 for anything that carries a token, once the switch is on."""
        if not REJECT_SESSION[0]:
            return False
        if self.path.startswith("/debug/"):
            return False
        self._send({"error": "unauthorized"}, status=401)
        return True

    def do_GET(self):
        if self._session_rejected():
            return
        if self.path.split("?")[0] == "/api/conversations/search":
            query = parse_qs(urlparse(self.path).query)
            term = query.get("q", [""])[0]
            # Deliberately NOT date order: the real endpoint hydrates by
            # walking the ranked hit ids, so the array arrives ranked.
            # The older thread comes first here, so a client that re-sorted
            # by date would visibly reorder it.
            # Both fixtures carry "ref 2026", so that term returns two
            # hits — and `reversed` puts the OLDER one first. A single-hit
            # fixture cannot tell a preserved ranking from a date sort,
            # which is how the first version of this test passed with the
            # client re-sorting by date.
            limit = int(query.get("limit", ["50"])[0])
            # "many" asks for more hits than the limit allows, so a
            # client can be shown to say when it is looking at a capped
            # result set rather than at everything that matched. Search
            # has no keyset parameter, so there is no next page and the
            # cap is the end of what can be seen.
            if term == "many":
                hits = [convo(f"m{i}", f"Many match {i}", "lots", 1754400000 - i)
                        for i in range(limit + 20)]
                self._send(hits[:limit])
                return
            hits = [c for c in reversed(CONVOS)
                    if term.lower() in c["subject"].lower() or term.lower() in c["snippet"].lower()]
            self._send(hits[:limit])
            return
        raw = re.match(r"^/api/mail/messages/(\d+)/raw$", self.path.split("?")[0])
        if raw:
            # `message/rfc822`, as the handler answers — a client that
            # expected JSON here would fail to decode a message that
            # arrived perfectly well.
            body = (
                f"Return-Path: <alice@example.com>\r\n"
                f"Received: from mx.example.com by mail.golia.jp;\r\n"
                f"Message-ID: <m{raw.group(1)}@x>\r\n"
                f"Subject: Quarterly report\r\n"
                f"\r\nplain fallback\r\n"
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "message/rfc822")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        attachment = re.match(
            r"^/api/mail/messages/(\d+)/attachments/(\d+)$", self.path.split("?")[0]
        )
        if attachment:
            index = int(attachment.group(2))
            uid = int(attachment.group(1))
            # Real messages carry their own part list; serve a valid
            # image for any of them so `cid:` resolution can be seen.
            for msgs in REAL_MESSAGES.values():
                if msgs and msgs[0]["uid"] == uid:
                    parts = msgs[0]["attachments"]
                    if index >= len(parts):
                        self._send({}, 404)
                        return
                    meta = parts[index]
                    self._send_bytes(PIXEL_PNG, meta["content_type"], meta["filename"])
                    return
            if index >= len(ATTACHMENTS):
                self._send({}, 404)
                return
            meta = ATTACHMENTS[index]
            FETCHED.append(index)
            self._send_bytes(PIXEL_PNG, meta["content_type"], meta["filename"])
            return
        if self.path.split("?")[0] == "/debug/fetched":
            self._send({"attachment_indices": FETCHED})
            return
        if self.path.split("?")[0] == "/api/conversations/unseen-count":
            UNSEEN_FETCHES[0] += 1
            self._send({"count": 3})
            return
        if self.path.split("?")[0] == "/debug/unseen-fetches":
            self._send({"fetches": UNSEEN_FETCHES[0]})
            return
        if self.path.split("?")[0] == "/api/scheduled":
            # Mail that has not left yet. `{"items": [...]}`, soonest
            # first — the shape `list_scheduled` answers with.
            self._send({"items": [
                {"id": "sch1", "scheduled_at": 1754500000,
                 "recipient": "alice@example.com", "subject": "Monday morning note"},
            ]})
            return
        if re.match(r"^/api/mail/sends/[^/]+/source$", self.path.split("?")[0]):
            # The bytes that left, as they left. `message/rfc822`, not
            # JSON — the client reads the body rather than decoding it.
            raw = ("From: me@golia.jp\r\n"
                   "To: carol@example.com\r\n"
                   "Subject: Never left the queue\r\n"
                   "Message-ID: <unfiled@golia.jp>\r\n\r\n"
                   "Trying again.\r\n")
            body = raw.encode()
            self.send_response(200)
            self.send_header("Content-Type", "message/rfc822")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(body)
            return
        if re.match(r"^/api/mail/sends/[^/]+/redraft$", self.path.split("?")[0]):
            # Compose fields plus attachment *metadata*: the bytes stay
            # here and the following send names what to keep by index.
            self._send({
                "redraft_of": "unfiled@golia.jp",
                "to": ["carol@example.com"], "cc": [], "bcc": [],
                "subject": "Never left the queue", "body": "Trying again.",
                "html_body": "", "in_reply_to": None,
                "attachments": [
                    {"index": 0, "filename": "invoice.pdf",
                     "content_type": "application/pdf", "size": 8192},
                ],
            })
            return
        if self.path.split("?")[0] == "/api/mail/sent":
            self._send([
                {"uid": 41, "message_id": "<filed@golia.jp>", "thread_id": "t1",
                 "to": "alice@example.com", "subject": "Filed and delivered",
                 "internal_date": 1754380000},
                {"uid": 42, "message_id": "<noproj@golia.jp>", "thread_id": "t2",
                 "to": "bob@example.com", "subject": "Predates the projection",
                 "internal_date": 1754370000},
            ])
            return
        members = re.match(r"^/api/admin/email-groups/(\d+)/members$", self.path.split("?")[0])
        if members:
            self._send({"members": GROUP_MEMBERS.get(int(members.group(1)), [])})
            return
        if self.path.split("?")[0] == "/api/admin/email-groups":
            self._send({"items": GROUPS})
            return
        perms = re.match(r"^/api/admin/groups/(\d+)/permissions$", self.path.split("?")[0])
        if perms:
            self._send({"permissions": GROUP_GRANTS.get(int(perms.group(1)), [])})
            return
        gmembers = re.match(r"^/api/admin/groups/(\d+)/members$", self.path.split("?")[0])
        if gmembers:
            self._send({"members": GROUP_PEOPLE.get(int(gmembers.group(1)), [])})
            return
        if self.path.split("?")[0] == "/api/admin/groups":
            self._send({"items": PERM_GROUPS})
            return
        if self.path.split("?")[0] == "/api/admin/permissions":
            self._send({"permissions": PERM_CATALOGUE})
            return
        if self.path.split("?")[0] == "/api/admin/audit-log":
            # The server filters by action PREFIX and scans a wider
            # window when it does; the fixture matches that contract so
            # a client that filtered locally would look identical here
            # and differ against the real one.
            wanted = parse_qs(urlparse(self.path).query).get("action", [""])[0]
            rows = [r for r in AUDIT if not wanted or r["action"].startswith(wanted)]
            self._send({"items": rows})
            return
        if self.path.split("?")[0] == "/api/admin/dmarc/reports":
            self._send({"items": DMARC_REPORTS})
            return
        if self.path.split("?")[0] == "/api/admin/dmarc/sources":
            self._send(DMARC_SOURCES)
            return
        if self.path.split("?")[0] == "/api/admin/queues":
            self._send({"items": queue_jobs()})
            return
        spam_list = re.match(r"^/api/spam/(whitelist|blacklist)$", self.path.split("?")[0])
        if spam_list:
            self._send({"entries": SPAM_LISTS[spam_list.group(1)]})
            return
        acct_side = re.match(
            r"^/api/admin/accounts/(.+)/(quota|sieve|webhook-subscriptions)$",
            self.path.split("?")[0],
        )
        if acct_side:
            who, what = unquote(acct_side.group(1)), acct_side.group(2)
            if what == "quota":
                match = next((a for a in ACCOUNTS if a["address"] == who), None)
                self._send({"quota_bytes": match["quota_bytes"] if match else None})
            elif what == "sieve":
                self._send({"script": SIEVE.get(who, "")})
            else:
                self._send({"items": WEBHOOKS.get(who, [])})
            return
        if self.path.split("?")[0] == "/api/admin/apps":
            self._send({"items": APPS})
            return
        if self.path.split("?")[0] == "/api/admin/suppressions":
            self._send({"items": SUPPRESSED})
            return
        if self.path.split("?")[0] == "/api/admin/aliases":
            self._send({"items": ALIASES})
            return
        if self.path.split("?")[0] == "/api/admin/accounts":
            self._send({"items": ACCOUNTS})
            return
        if self.path.split("?")[0] == "/api/admin/domains":
            self._send({"items": DOMAINS})
            return
        if self.path.split("?")[0] == "/api/contacts":
            # Same shape as get_contacts: bare array of "Name <email>",
            # substring match on either half, case-insensitive.
            qs = parse_qs(urlparse(self.path).query)
            q = qs.get("q", [""])[0].lower()
            CONTACT_QUERIES.append(q)
            book = ["Alice Smith <alice@example.com>",
                    "Bob <bob@example.com>",
                    "Keiri <keiri@example.co.jp>"]
            self._send([c for c in book if q in c.lower()])
            return
        if self.path == "/debug/contact-queries":
            self._send({"queries": CONTACT_QUERIES})
            return
        if self.path.split("?")[0] == "/api/mail/sends":
            # One joined (brackets deliberately absent — the join must
            # normalise), one failed send the sweep has not filed.
            self._send([
                {"send_id": "filed@golia.jp", "thread_id": "t1",
                 "subject": "Filed and delivered", "to": ["alice@example.com"],
                 "created_at": 1754380000, "status": "delivered",
                 "can_resend": False, "resent_from": None, "recipients": []},
                {"send_id": "unfiled@golia.jp", "thread_id": "t3",
                 "subject": "Never left the queue", "to": ["carol@example.com"],
                 "created_at": 1754390000, "status": "failed",
                 "can_resend": True, "resent_from": None, "recipients": []},
            ])
            return
        if self.path.split("?")[0] == "/api/agent/keys":
            self._send({"items": AGENT_KEYS})
            return
        if self.path.split("?")[0] == "/api/mail/signatures":
            # A bare array, as `list_signatures` answers it. Two of
            # them, because the client has to pick the default rather
            # than the first — and a single-signature fixture cannot
            # tell those apart.
            self._send([
                {"id": 1, "name": "Short", "html": "", "text_content": "Sent from a phone",
                 "is_default": False, "created_at": ""},
                {"id": 2, "name": "Work", "html": "", "text_content": "Li Hao\nGOLIA",
                 "is_default": True, "created_at": ""},
            ])
            return
        if self.path.split("?")[0] == "/api/mail/drafts":
            # The delay covers drafts too: the sheet's spinner is only
            # observable while the request is out, and without it the
            # stub answers faster than XCUITest can look — which is how
            # the sheet shipped announcing "No drafts" over a full list.
            if LIST_DELAY_MS[0]:
                time.sleep(LIST_DELAY_MS[0] / 1000)
            self._send(sorted(DRAFTS.values(), key=lambda d: -d["updated_at"]))
            return
        if self.path.split("?")[0] == "/debug/phish":
            PHISH_HITS[0] += 1
            self._send({"hits": PHISH_HITS[0]})
            return
        if self.path.split("?")[0] == "/debug/phish-hits":
            self._send({"hits": PHISH_HITS[0]})
            return
        if self.path.split("?")[0] == "/debug/account-posts":
            self._send({"posts": ACCOUNT_POSTS})
            return
        if self.path.split("?")[0] == "/debug/list-fetches":
            self._send({"fetches": LIST_FETCHES[0]})
            return
        if self.path.split("?")[0] == "/debug/writes":
            self._send({"writes": WRITES})
            return
        if self.path.split("?")[0] == "/debug/draft-posts":
            self._send({"ids": DRAFT_POSTS})
            return
        if self.path.split("?")[0] == "/debug/sent":
            self._send({"sent": SENT})
            return
        if self.path.split("?")[0] == "/debug/verbs":
            self._send({"verbs": VERBS})
            return
        if self.path.split("?")[0] == "/debug/unsubscribed":
            self._send({"unsubscribed": UNSUBSCRIBED})
            return
        _path = self.path.split("?")[0]
        if re.match(r"^/api/conversations/t\d+$", _path) or _path.rsplit("/", 1)[-1] in REAL_MESSAGES:
            # The delay covers thread bodies too: the offline tests need
            # a window in which only a cache could have painted them.
            if LIST_DELAY_MS[0]:
                time.sleep(LIST_DELAY_MS[0] / 1000)
            tid = _path.rsplit("/", 1)[-1]
            if tid in REAL_MESSAGES:
                self._send(REAL_MESSAGES[tid])
                return
            if self.path.split("?")[0] == "/api/conversations/t2":
                self._send(ALIAS_THREAD)
                return
            if self.path.split("?")[0] == "/api/conversations/t3":
                self._send(NEWSLETTER)
                return
            self._send(MESSAGES)
        elif self.path.startswith("/api/conversations"):
            LIST_FETCHES[0] += 1
            if LIST_DELAY_MS[0]:
                time.sleep(LIST_DELAY_MS[0] / 1000)
            query = parse_qs(urlparse(self.path).query)
            # The paging fixture is opt-in so the small, readable
            # two-row list stays what the other tests see.
            folder = query.get("folder", [""])[0]
            archived = query.get("archived", ["false"])[0] == "true"
            # Each list gets its own row, so a switch that did not change
            # what was asked for would show the wrong one. Without this
            # every folder returned the same two threads and a broken
            # switcher looked correct.
            if archived:
                self._send([convo("arch1", "Archived thread", "old news", 1754100000)])
                return
            if folder == "Junk":
                self._send([convo("junk1", "You have won", "definitely real", 1754200000)])
                return
            if folder == "NonJunk" and query.get("starred", [""])[0] == "true":
                self._send([convo("star1", "Starred thread", "kept", 1754250000)])
                return
            if folder == "NonJunk" and query.get("unread", [""])[0] == "true":
                self._send([convo("unread1", "Unread thread", "not opened", 1754260000)])
                return
            if folder == "NP":
                # t3 rides along so a client whose folder list has no
                # test-only "Lists" entry can still reach the newsletter
                # — a newsletter is what N & P is for. The `Lists` branch
                # below stays for the iOS suite, which asks for it by
                # name.
                self._send([convo("np1", "Newsletter thread", "weekly", 1754270000),
                            convo("t3", "This week in systems design",
                                  "the one with a way out", 1754280000)])
                return
            # A list the newsletter thread is reachable from, so the
            # unsubscribe footer can be opened without disturbing the
            # two-row list every other test reads.
            if folder == "Lists":
                self._send([convo("t3", "This week in systems design",
                                  "the one with a way out", 1754280000)])
                return
            if folder == "Dense":
                rows = [convo(f"d{i}",
                              ["Quarterly planning follow-up", "請求書のご送付につきまして",
                               "Re: server maintenance window", "Team offsite logistics",
                               "Invoice #2026-081 overdue", "Weekly metrics digest",
                               "Your parcel is out for delivery", "会議室予約の確認",
                               "Password rotation reminder", "Q3 budget review notes",
                               "New starter onboarding", "Renewal quote attached"][i % 12],
                              "preview text that would wrap",
                              int(time.time()) - [3600, 7200, 90000, 260000, 350000,
                                                  500000, 700000, 2600000, 5200000,
                                                  9000000, 34000000, 40000000][i % 12])
                        for i in range(12)]
                for i, r in enumerate(rows):
                    r["unread_count"] = 1 if i % 3 == 0 else 0
                    r["flagged"] = i % 4 == 0
                    r["message_count"] = (i % 5) + 1
                self._send(rows)
                return
            if folder == "Paged":
                limit = int(query.get("limit", ["50"])[0])
                before = query.get("before_ts", [None])[0]
                self._send(_paged_convos(limit, int(before) if before else None))
            else:
                # Real threads first when the door is open, so the mail
                # under inspection is the first thing on screen.
                self._send(REAL_THREADS + CONVOS)
        else:
            self._send([], 404)

    def log_message(self, *a):
        pass

    def do_PUT(self):
        perms = re.match(r"^/api/admin/groups/(\d+)/permissions$", self.path.split("?")[0])
        if perms:
            WRITES.append(f"PUT {self.path.split('?')[0]}")
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            # Replace, exactly as the handler does: a client that sent a
            # delta would look right here and silently revoke the rest
            # against the real server.
            GROUP_GRANTS[int(perms.group(1))] = body.get("permissions", [])
            self._send({"ok": True})
            return
        self._send({}, 404)

    def do_DELETE(self):
        key = re.match(r"^/api/agent/keys/(\d+)$", self.path.split("?")[0])
        if key:
            WRITES.append("DELETE /api/agent/keys/" + key.group(1))
            wanted = int(key.group(1))
            AGENT_KEYS[:] = [k for k in AGENT_KEYS if k["id"] != wanted]
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        WRITES.append("DELETE " + self.path.split("?")[0])
        spam_list = re.match(r"^/api/spam/(whitelist|blacklist)$", self.path.split("?")[0])
        if spam_list:
            self._send({"entries": SPAM_LISTS[spam_list.group(1)]})
            return
        acct_side = re.match(
            r"^/api/admin/accounts/(.+)/(quota|sieve|webhook-subscriptions)$",
            self.path.split("?")[0],
        )
        if acct_side:
            who, what = unquote(acct_side.group(1)), acct_side.group(2)
            if what == "quota":
                match = next((a for a in ACCOUNTS if a["address"] == who), None)
                self._send({"quota_bytes": match["quota_bytes"] if match else None})
            elif what == "sieve":
                self._send({"script": SIEVE.get(who, "")})
            else:
                self._send({"items": WEBHOOKS.get(who, [])})
            return
        if self.path.split("?")[0] == "/api/admin/apps":
            self._send({"items": APPS})
            return
        if self.path.split("?")[0] == "/api/admin/suppressions":
            SUPPRESSED.clear()
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        member = re.match(r"^/api/admin/email-groups/(\d+)/members/(.+)$",
                          self.path.split("?")[0])
        if member:
            gid, addr = int(member.group(1)), unquote(member.group(2))
            GROUP_MEMBERS[gid] = [m for m in GROUP_MEMBERS.get(gid, []) if m != addr]
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        group = re.match(r"^/api/admin/email-groups/(\d+)$", self.path.split("?")[0])
        if group:
            wanted = int(group.group(1))
            GROUPS[:] = [g for g in GROUPS if g["id"] != wanted]
            GROUP_MEMBERS.pop(wanted, None)
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        account = re.match(r"^/api/admin/accounts/(.+)$", self.path.split("?")[0])
        if account:
            wanted = unquote(account.group(1))
            ACCOUNTS[:] = [a for a in ACCOUNTS if a["address"] != wanted]
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        domain = re.match(r"^/api/admin/domains/(.+)$", self.path.split("?")[0])
        if domain:
            wanted = unquote(domain.group(1))
            DOMAINS[:] = [d for d in DOMAINS if d["name"] != wanted]
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        alias = re.match(r"^/api/admin/aliases/(\d+)$", self.path.split("?")[0])
        if alias:
            wanted = int(alias.group(1))
            ALIASES[:] = [a for a in ALIASES if a["id"] != wanted]
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        spam_entry = re.match(r"^/api/spam/(whitelist|blacklist)/(.+)$", self.path.split("?")[0])
        if spam_entry:
            kind, address = spam_entry.group(1), unquote(spam_entry.group(2))
            SPAM_LISTS[kind][:] = [a for a in SPAM_LISTS[kind] if a != address]
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        draft = re.match(r"^/api/mail/drafts/(\d+)$", self.path.split("?")[0])
        if draft:
            DRAFTS.pop(int(draft.group(1)), None)
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        # `DELETE /api/conversations/{id}` — 204, no body. The real one
        # unlinks maildir files; this one just says yes.
        if re.match(r"^/api/conversations/[\w-]+$", self.path.split("?")[0]):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        else:
            self._send({}, 404)

    def do_POST(self):
        if self._session_rejected():
            return
        if re.match(r"^/api/mail/sends/[^/]+/resend$", self.path.split("?")[0]):
            WRITES.append("POST " + self.path.split("?")[0])
            self._send({"send_id": "resent@golia.jp"})
            return
        if self.path.split("?")[0] == "/api/conversations/mark-all-read":
            # The **whole** path, query and all. Everywhere else the
            # query is dropped, and here it is the assertion: the same
            # four axes the list takes decide whether this marks one
            # list or the entire mailbox.
            WRITES.append("POST " + self.path)
            self._send({"marked": 3})
            return
        if re.match(r"^/api/scheduled/[^/]+/cancel$", self.path.split("?")[0]):
            # Calling a message back before it leaves. Recorded, because
            # the row goes from the list optimistically and the only
            # evidence the request happened is here.
            WRITES.append("POST " + self.path.split("?")[0])
            self._send({"ok": True})
            return
        if self.path.split("?")[0] == "/api/agent/keys":
            WRITES.append("POST /api/agent/keys")
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            AGENT_KEY_COUNTER[0] += 1
            new_id = AGENT_KEY_COUNTER[0]
            secret = f"mk_{new_id:06d}deadbeefcafe1234"
            AGENT_KEYS.append({
                "id": new_id, "name": body.get("name", ""),
                "scopes": body.get("scopes", []), "prefix": secret[:8],
                "created_at": 1754400100,
            })
            # The secret travels exactly once, as the handler does it.
            self._send({"id": new_id, "secret": secret})
            return
        # Counted before anything else: a post that reaches here came out
        # of a rendered message body, and the client is supposed to make
        # that impossible.
        if self.path.split("?")[0] == "/debug/phish":
            PHISH_HITS[0] += 1
            self._send({"hits": PHISH_HITS[0]})
            return
        # The reset itself is not traffic under test — and it must not be
        # recorded before clearing, or clear-then-unrecord pops an empty
        # list.
        if not self.path.startswith("/debug/"):
            WRITES.append("POST " + self.path.split("?")[0])
        # Each test starts from a clean recorder. Without this the lists
        # accumulate across the whole run and an assertion like "exactly
        # one send" passes or fails on test order — which is how the
        # reply test started seeing the compose test's message.
        if self.path.split("?")[0] == "/debug/set-delay":
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            LIST_DELAY_MS[0] = int(body.get("ms", 0))
            self._send({"ok": True})
            return
        if self.path.split("?")[0] == "/debug/reset":
            # Including the session switch: a test that turned it on
            # would otherwise 401 every test after it, and the failure
            # would look like anything but a leftover flag.
            REJECT_SESSION[0] = False
            VERB_REFUSE.clear()
            FETCHED.clear()
            SENT.clear()
            DRAFTS.clear()
            DRAFT_COUNTER[0] = 0
            DRAFT_POSTS.clear()
            WRITES.clear()
            UNSEEN_FETCHES[0] = 0
            LIST_FETCHES[0] = 0
            LIST_DELAY_MS[0] = 0
            PHISH_HITS[0] = 0
            AGENT_KEYS[:] = [
                {"id": 1, "name": "Scheduler", "scopes": ["mail.send"],
                 "prefix": "mk_a1b2c", "created_at": 1754400000},
            ]
            AGENT_KEY_COUNTER[0] = 1
            CONTACT_QUERIES.clear()
            ALIASES[:] = [
                {"id": 1, "source_address": "sales@golia.jp",
                 "target_address": "me@golia.jp", "domain": "golia.jp",
                 "alias_type": "alias", "active": True, "created_at": 1754400000},
                {"id": 2, "source_address": "info@golia.ai",
                 "target_address": "lihao@golia.jp", "domain": "golia.ai",
                 "alias_type": "alias", "active": False, "created_at": 1754400001},
            ]
            ALIAS_COUNTER[0] = 2
            ACCOUNTS[:] = [
                {"address": "lihao@golia.jp", "domain": "golia.jp",
                 "display_name": "Li Hao", "active": True,
                 "created_at": 1754400000, "quota_bytes": 5368709120},
                {"address": "noreply@golia.jp", "domain": "golia.jp",
                 "display_name": "", "active": False,
                 "created_at": 1754400001, "quota_bytes": 0},
            ]
            DOMAINS[:] = [
                {"name": "golia.jp", "created_at": 1754400000},
                {"name": "golia.ai", "created_at": 1754400001},
            ]
            ACCOUNT_POSTS.clear()
            GROUPS[:] = [
                {"id": 1, "address": "team@golia.jp", "domain": "golia.jp",
                 "name": "Team", "description": "", "created_at": 1754400000},
            ]
            GROUP_MEMBERS.clear()
            GROUP_MEMBERS[1] = ["lihao@golia.jp", "Keiri <keiri@golia.jp>"]
            GROUP_COUNTER[0] = 1
            SUPPRESSED[:] = ["bounced@example.com", "closed@example.com"]
            UNSUBSCRIBED.clear()
            VERBS.clear()
            VERB_REFUSE.clear()
            UNSUB_REFUSE[0] = False
            GROUP_GRANTS.clear()
            GROUP_GRANTS.update({1: ["admin.accounts", "admin.aliases"], 2: ["mail.read"]})
            self._send({"ok": True})
            return
        if self.path.split("?")[0] == "/api/conversations/batch":
            # One request for many threads, answering *which* ids it
            # could not do. The count-only answer this replaced is why
            # the client used to send one request per row: a batch that
            # half-worked has to put back its own half.
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            action = body.get("action", "")
            ids = body.get("thread_ids", [])
            refused = [tid for tid in ids if action in VERB_REFUSE]
            for tid in ids:
                if tid not in refused:
                    VERBS.append(f"{action} {tid}")
            self._send({
                "failed": len(refused),
                "failed_thread_ids": refused,
                "processed": len(ids) - len(refused),
                "success": not refused,
            })
            return
        if re.match(
            r"^/api/conversations/[\w%.@-]+/(read|unread|star|unstar|archive|unarchive"
            r"|mark-junk|mark-not-junk|mark-notification|mark-promotion|move-to-inbox)$",
            self.path.split("?")[0],
        ):
            parts = self.path.split("?")[0].split("/")
            VERBS.append(f"{parts[-1]} {parts[-2]}")
            # A verb the test asked to be refused, so the client's
            # failure path can be looked at rather than reasoned about.
            if parts[-1] in VERB_REFUSE:
                self.send_response(500)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif self.path.startswith("/api/auth/login"):
            self._send({"address": "me@golia.jp", "display_name": "Me",
                        "permissions": [], "token": "stub-token"})
        elif self.path.split("?")[0].startswith("/debug/refuse-verb/"):
            VERB_REFUSE.add(self.path.rsplit("/", 1)[-1])
            self._send({"ok": True})
        elif self.path.split("?")[0] == "/debug/reject-session":
            REJECT_SESSION[0] = True
            self._send({"ok": True})
        elif self.path.split("?")[0] == "/debug/unsubscribe-refuse":
            UNSUB_REFUSE[0] = True
            self._send({"ok": True})
        elif self.path.split("?")[0] == "/api/mail/unsubscribe":
            # The real endpoint takes a message, looks up that message's
            # own List-Unsubscribe header and posts to it. A body
            # carrying a URL would mean the server had become a request
            # forwarder — so a URL here is a 400, and a test can prove
            # the client never sends one.
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            if any(k in body for k in ("url", "http", "target")):
                self._send({"ok": False, "message": "the body names a message, not a URL"},
                            status=400)
                return
            UNSUBSCRIBED.append({"thread_id": body.get("thread_id"), "uid": body.get("uid")})
            if UNSUB_REFUSE[0]:
                self._send({"ok": False, "status": 500})
                return
            self._send({"ok": True, "status": 200})
        elif self.path.startswith("/api/push/tokens"):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif self.path.startswith("/api/mail/drafts"):
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            DRAFT_POSTS.append(body.get("id"))
            draft_id = body.get("id")
            if draft_id is None:
                DRAFT_COUNTER[0] += 1
                draft_id = DRAFT_COUNTER[0]
            now = 1754400000 + draft_id
            DRAFTS[draft_id] = {
                "id": draft_id, "to": body.get("to", ""), "cc": body.get("cc", ""),
                "bcc": body.get("bcc", ""), "subject": body.get("subject", ""),
                "body": body.get("body", ""),
                "reply_to_thread_id": body.get("reply_to_thread_id"),
                "created_at": now, "updated_at": now,
            }
            self._send({"id": draft_id})
        elif re.match(r"^/api/admin/email-groups/(\d+)/members$", self.path.split("?")[0]):
            gid = int(re.match(r"^/api/admin/email-groups/(\d+)/members$",
                               self.path.split("?")[0]).group(1))
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            GROUP_MEMBERS.setdefault(gid, []).append(body.get("member_address", ""))
            self._send({"ok": True})
        elif self.path.split("?")[0] == "/api/admin/email-groups":
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            GROUP_COUNTER[0] += 1
            GROUPS.append({
                "id": GROUP_COUNTER[0], "address": body.get("address", ""),
                "domain": body.get("domain", ""), "name": body.get("name", ""),
                "description": body.get("description", ""), "created_at": 1754400002,
            })
            GROUP_MEMBERS[GROUP_COUNTER[0]] = []
            self._send({"id": GROUP_COUNTER[0]})
        elif self.path.split("?")[0] == "/api/admin/accounts":
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            addr = body.get("address", "")
            ACCOUNTS.append({
                "address": addr,
                "domain": addr.split("@")[-1] if "@" in addr else "",
                "display_name": body.get("display_name", ""),
                "active": True, "created_at": 1754400002, "quota_bytes": 0,
            })
            # The password must reach the server and go no further: the
            # recorder keeps whether one arrived, never the value.
            ACCOUNT_POSTS.append({"address": addr,
                                  "had_password": bool(body.get("password"))})
            self._send({"ok": True})
        elif self.path.split("?")[0] == "/api/admin/domains":
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            DOMAINS.append({"name": body.get("name", ""), "created_at": 1754400002})
            self._send({"ok": True})
        elif re.match(r"^/api/spam/(whitelist|blacklist)$", self.path.split("?")[0]):
            kind = re.match(r"^/api/spam/(whitelist|blacklist)$", self.path.split("?")[0]).group(1)
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            address = body.get("address", "")
            if address and address not in SPAM_LISTS[kind]:
                SPAM_LISTS[kind].append(address)
            self._send({"ok": True})
        elif self.path.split("?")[0] == "/api/admin/aliases":
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length)) if length else {}
            ALIAS_COUNTER[0] += 1
            ALIASES.append({
                "id": ALIAS_COUNTER[0],
                "source_address": body.get("source_address", ""),
                "target_address": body.get("target_address", ""),
                "domain": body.get("domain", ""),
                "alias_type": body.get("alias_type", ""),
                "active": True, "created_at": 1754400002,
            })
            self._send({"id": ALIAS_COUNTER[0]})
        elif self.path.split("?")[0] == "/api/mail/send-multipart":
            # Parsed with the stdlib email machinery: prepend the real
            # Content-Type header and the form body is a MIME multipart.
            # Recorded in the same shape as the JSON sends, with the
            # files summarised — filename, declared type, byte count —
            # so tests assert on what arrived, not what was meant.
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length)
            from email.parser import BytesParser
            msg = BytesParser().parsebytes(
                b"Content-Type: " + self.headers.get("Content-Type", "").encode() +
                b"\r\n\r\n" + raw)
            record = {"to": [], "attachments": []}
            for part in msg.walk():
                if part.is_multipart():
                    continue
                name = part.get_param("name", header="content-disposition")
                filename = part.get_filename()
                payload = part.get_payload(decode=True) or b""
                if filename:
                    record["attachments"].append({
                        "filename": filename,
                        "content_type": part.get_content_type(),
                        "bytes": len(payload),
                    })
                elif name == "to":
                    record["to"].append(payload.decode())
                elif name:
                    record[name] = payload.decode()
            SENT.append(record)
            self._send({"message_id": "<stub-multipart@golia.jp>", "success": True})
        elif self.path.startswith("/api/mail/send"):
            # The body is recorded whole, `forward_attachments_from`
            # included: a forward that dropped it would look identical
            # on screen and arrive without the attachments it was
            # forwarding.
            length = int(self.headers.get("Content-Length", "0"))
            if length:
                try:
                    SENT.append(json.loads(self.rfile.read(length)))
                except ValueError:
                    SENT.append({})
            # `success` is part of the answer, not just the status code:
            # the handler returns 200 with `success: false` for a message
            # it accepted but could not queue.
            self._send({"message_id": "<stub-reply@golia.jp>", "success": True})
        else:
            self._send({}, 404)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 6039
    # **A deeper listen backlog, set before the socket is bound.**
    # socketserver defaults to 5, and this is HTTP/1.0 — one connection
    # per request — so a client that fires a burst overflows it and the
    # kernel drops the extra SYNs. The client sees "unexpected end of
    # stream", which reads as the server crashing and is really a queue
    # that was five deep. It only appeared once the Android suite
    # stopped crossing the emulator's NAT and started connecting at
    # local speed. It has to be set on the class: `listen()` happens in
    # the constructor, and setting it afterwards changes nothing.
    ThreadingHTTPServer.request_queue_size = 128
    ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
