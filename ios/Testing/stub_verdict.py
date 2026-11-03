"""Fraud verdicts, sender checks and link warnings on the reading page.

`GET /api/messages/{message_id}/fraud-verdict` answers as
`crates/webapi/src/handlers/quarantine.rs` does: `{"verdict": null}` for
almost every message. `POST /debug/verdict` arms the suspicious case for
the ordinary thread (`t1` and the threads served like it): the second
message, from a suspicious sender, then carries a held verdict across the
four layers, its SPF/DKIM/DMARC results, a link whose text shows another
host and a callback number; the first message carries only why it was
filed where it was. Armed only on request, so the suites that read this
thread for other reasons see it unchanged. `/debug/reset` disarms it, and
so does `POST /debug/verdict?on=0`.
"""

import copy
import re
from urllib.parse import parse_qs, unquote, urlparse

ARMED = [False]

HELD_MESSAGE = "<m2@x>"
FILED_MESSAGE = "<m1@x>"

HELD = {
    "layers": [
        {"name": "transport", "outcome": "fail", "score": 2.0,
         "detail": "nothing vouches for the sender",
         "reasons": [{"code": "nothing-vouches-for-the-sender"}]},
        {"name": "identity", "outcome": "fail", "score": 4.0,
         "detail": "display name claims Golia",
         "reasons": [{"code": "claims-our-name", "params": {"name": "Golia"}}]},
        {"name": "provenance", "outcome": "not_applicable", "score": 0.0, "detail": ""},
        {"name": "content", "outcome": "pass", "score": 0.0, "detail": "", "reasons": []},
    ],
    "quarantined": True,
    "rules_version": "2026-11-03.1",
    "score": 6.0,
}

# mail nobody suspected, filed by the classifier
FILED = {
    "layers": [],
    "placement": [{"code": "sorted-by-classifier", "params": {"class": "promotion"}}],
    "quarantined": False,
    "rules_version": "2026-11-03.1",
    "score": 0.0,
}

LINKS_HTML = (
    "<p>Short reply, narrow body.</p>"
    '<p>Sign in at <a href="https://webmail-oauth.wzldq.com/auth">www.golia.jp/login</a></p>'
    '<p><a href="https://example.org/help">the help page</a></p>'
    "<p>Questions? Call +81 3-1234-5678.</p>"
)

SUSPICIOUS = {
    "html_body": LINKS_HTML,
    "text_body": "Short reply, narrow body. Questions? Call +81 3-1234-5678.",
    "sender_checks": [
        {"code": "auth-check", "params": {"check": "SPF", "result": "pass"}},
        {"code": "auth-check", "params": {"check": "DKIM", "result": "fail"}},
        {"code": "auth-check", "params": {"check": "DMARC", "result": "fail"}},
    ],
    "link_warnings": [
        {"host": "webmail-oauth.wzldq.com", "shown": "www.golia.jp",
         "reasons": [
             {"code": "link-shows-another-host",
              "params": {"shown": "www.golia.jp", "host": "webmail-oauth.wzldq.com"}},
             {"code": "link-host-listed", "params": {"host": "webmail-oauth.wzldq.com"}},
         ]},
    ],
}


def arm(handler):
    """Handle `POST /debug/verdict[?on=0]`; True when this was it."""
    if handler.path.split("?")[0] != "/debug/verdict":
        return False
    ARMED[0] = parse_qs(urlparse(handler.path).query).get("on", ["1"])[0] != "0"
    handler._send({"ok": True})
    return True


def get(handler, messages):
    """The verdict route, and the armed thread; True when this answered."""
    path = handler.path.split("?")[0]
    m = re.match(r"^/api/messages/([^/]+)/fraud-verdict$", path)
    if m:
        handler._send({"verdict": _verdict(unquote(m.group(1)))})
        return True
    if ARMED[0] and re.match(r"^/api/conversations/t\d+$", path) and path[-3:] not in ("/t2", "/t3"):
        handler._send([_decorated(msg) for msg in messages])
        return True
    return False


def _verdict(message_id):
    if not ARMED[0]:
        return None
    if message_id == HELD_MESSAGE:
        return HELD
    if message_id == FILED_MESSAGE:
        return FILED
    return None


def _decorated(msg):
    out = copy.deepcopy(msg)
    out.setdefault("sender_checks", [])
    out.setdefault("link_warnings", [])
    if out["message_id"] == HELD_MESSAGE:
        out.update(copy.deepcopy(SUSPICIOUS))
    return out


def reset():
    ARMED[0] = False
