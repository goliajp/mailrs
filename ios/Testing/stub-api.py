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
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

WIDE = ('<table width="760" style="width:760px"><tr><td>'
        '<div style="width:760px;background:#eef;padding:8px">'
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

CONVOS = [{
    "thread_id": "t1", "subject": "Quarterly report and the follow-up notes",
    "participants": ["alice@example.com"], "message_count": 2, "unread_count": 2,
    "last_date": 1754400000, "category": "inbox", "flagged": False,
    "snippet": "Please review before Friday", "pinned": False, "archived": False,
    "importance_level": "normal", "importance_score": 0.5, "requires_action": False,
    "received_count": 2, "sent_count": 0,
}, {
    "thread_id": "t2", "subject": "請求書のご送付につきまして",
    "participants": ["keiri@example.co.jp"], "message_count": 1, "unread_count": 0,
    "last_date": 1754300000, "category": "inbox", "flagged": False,
    "snippet": "ご確認ください。", "pinned": False, "archived": False,
    "importance_level": "normal", "importance_score": 0.2, "requires_action": False,
    "received_count": 1, "sent_count": 0,
}]

def msg(uid, sender, trust, html):
    return {"uid": uid, "sender": sender, "sender_trust": trust,
            "recipients": "me@golia.jp", "subject": "Quarterly report", "flags": 0,
            "internal_date": 1754400000, "message_id": f"<m{uid}@x>",
            "text_body": "plain fallback", "html_body": html, "attachments": [],
            "category": "inbox", "risk_score": 0, "risk_reason": "", "summary": "",
            "people": {}, "dates": {}, "amounts": {}, "action_items": [],
            "ai_analyzed": False, "importance_level": "normal", "importance_score": 0.1,
            "is_bulk_sender": False, "has_tracking_pixel": False,
            "requires_action": False, "sender_intent": ""}

MESSAGES = [msg(1, "alice@example.com", "verified", WIDE),
            msg(2, "spoofed@example.com", "suspicious", "<p>Short reply, narrow body.</p>")]

class H(BaseHTTPRequestHandler):
    def _send(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if re.match(r"^/api/conversations/t\d+$", self.path.split("?")[0]):
            self._send(MESSAGES)
        elif self.path.startswith("/api/conversations"):
            query = parse_qs(urlparse(self.path).query)
            # The paging fixture is opt-in so the small, readable
            # two-row list stays what the other tests see.
            if query.get("folder", [""])[0] == "Paged":
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
        # `DELETE /api/conversations/{id}` — 204, no body. The real one
        # unlinks maildir files; this one just says yes.
        if re.match(r"^/api/conversations/[\w-]+$", self.path.split("?")[0]):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        else:
            self._send({}, 404)

    def do_POST(self):
        if re.match(r"^/api/conversations/[\w-]+/(un)?archive$", self.path.split("?")[0]):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif self.path.startswith("/api/auth/login"):
            self._send({"address": "me@golia.jp", "display_name": "Me",
                        "permissions": [], "token": "stub-token"})
        elif self.path.startswith("/api/mail/send"):
            # `success` is part of the answer, not just the status code:
            # the handler returns 200 with `success: false` for a message
            # it accepted but could not queue.
            self._send({"message_id": "<stub-reply@golia.jp>", "success": True})
        else:
            self._send({}, 404)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 6039
    HTTPServer(("127.0.0.1", port), H).serve_forever()
