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
    {"id": 1, "source_address": "sales@golia.jp", "target_address": "lihao@golia.jp",
     "domain": "golia.jp", "alias_type": "alias", "active": True, "created_at": 1754400000},
    {"id": 2, "source_address": "info@golia.ai", "target_address": "lihao@golia.jp",
     "domain": "golia.ai", "alias_type": "alias", "active": False, "created_at": 1754400001},
]
ALIAS_COUNTER = [2]

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

class H(BaseHTTPRequestHandler):
    def _send(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
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

    def do_GET(self):
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
            hits = [c for c in reversed(CONVOS)
                    if term.lower() in c["subject"].lower() or term.lower() in c["snippet"].lower()]
            self._send(hits)
            return
        attachment = re.match(
            r"^/api/mail/messages/(\d+)/attachments/(\d+)$", self.path.split("?")[0]
        )
        if attachment:
            index = int(attachment.group(2))
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
        if self.path.split("?")[0] == "/api/mail/drafts":
            self._send(sorted(DRAFTS.values(), key=lambda d: -d["updated_at"]))
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
        if re.match(r"^/api/conversations/t\d+$", self.path.split("?")[0]):
            # The delay covers thread bodies too: the offline tests need
            # a window in which only a cache could have painted them.
            if LIST_DELAY_MS[0]:
                time.sleep(LIST_DELAY_MS[0] / 1000)
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
                self._send([convo("np1", "Newsletter thread", "weekly", 1754270000)])
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
                self._send(CONVOS)
        else:
            self._send([], 404)

    def log_message(self, *a):
        pass

    def do_DELETE(self):
        WRITES.append("DELETE " + self.path.split("?")[0])
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
            FETCHED.clear()
            SENT.clear()
            DRAFTS.clear()
            DRAFT_COUNTER[0] = 0
            DRAFT_POSTS.clear()
            WRITES.clear()
            UNSEEN_FETCHES[0] = 0
            LIST_FETCHES[0] = 0
            LIST_DELAY_MS[0] = 0
            CONTACT_QUERIES.clear()
            ALIASES[:] = [
                {"id": 1, "source_address": "sales@golia.jp",
                 "target_address": "lihao@golia.jp", "domain": "golia.jp",
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
            self._send({"ok": True})
            return
        if re.match(
            r"^/api/conversations/[\w-]+/(read|unread|star|unstar|archive|unarchive|mark-junk|mark-not-junk)$",
            self.path.split("?")[0],
        ):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif self.path.startswith("/api/auth/login"):
            self._send({"address": "me@golia.jp", "display_name": "Me",
                        "permissions": [], "token": "stub-token"})
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
    ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
