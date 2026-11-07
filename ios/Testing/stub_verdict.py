"""Fraud verdicts, sender checks and link warnings on the reading page.

`GET /api/messages/{message_id}/fraud-verdict` answers as
`crates/webapi/src/handlers/quarantine.rs` does: `{"verdict": null}` for
almost every message. `POST /debug/verdict` arms the suspicious case for
the ordinary thread (`t1` and the threads served like it): the second
message, from a suspicious sender, then carries a held verdict across the
four layers, its SPF/DKIM/DMARC results, a link whose text shows another
host and two phone numbers; the first message carries only why it was
filed where it was. Armed only on request, so the suites that read this
thread for other reasons see it unchanged. `/debug/reset` disarms it, and
so does `POST /debug/verdict?on=0`.

`GET /api/senders/{address}/callback-numbers` answers the numbers the
sender gave the reader before. Armed, the held message's sender gave
`0120-111-222` in another conversation (so the callback notice leaves it
out), and the history already holds `+81 3-1234-5678` as first given by
the held message itself (so the notice still names it: a client that took
every number in the history as old would hide it). Unarmed, every sender
gave none.
"""

import copy
import re
from urllib.parse import parse_qs, unquote, urlparse

ARMED = [False]

HELD_MESSAGE = "<m2@x>"
FILED_MESSAGE = "<m1@x>"

# as `mailrs_inbound::assess` stores it: every finding a line of the layer
# it was filed under, each scored line carrying what it added, so a layer's
# score is the sum of its lines and the verdict's the sum of the layers.
# Transport was never checked (a re-scan of mail with no
# Authentication-Results); provenance looked and found nothing.
HELD = {
    "layers": [
        {"name": "transport", "outcome": "not_applicable", "score": 0.0, "detail": ""},
        {"name": "identity", "outcome": "fail", "score": 6.0,
         "detail": "display name claims Golia; nothing vouches for the sender",
         "reasons": [
             {"code": "claims-our-name", "params": {"name": "Golia"}, "score": 4.0},
             {"code": "nothing-vouches-for-the-sender", "score": 2.0},
         ]},
        {"name": "provenance", "outcome": "pass", "score": 0.0,
         "detail": "nothing unusual in the headers", "reasons": []},
        {"name": "content", "outcome": "fail", "score": 6.0,
         "detail": "content 0.0, classifier 0.0; the text asks the reader to send their own LINE QR code",
         "reasons": [{"code": "asks-for-the-readers-line", "score": 6.0}]},
    ],
    "quarantined": True,
    "rules_version": "2026-11-03.1",
    "score": 12.0,
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
    "<p>Our office: 0120-111-222.</p>"
)

SUSPICIOUS = {
    "html_body": LINKS_HTML,
    "text_body": "Short reply, narrow body. Questions? Call +81 3-1234-5678. Our office: 0120-111-222.",
    # what `nothing-vouches-for-the-sender` in the identity layer fires on:
    # no DKIM signature and an SPF that did not pass, under a DMARC record
    # (the forged sender of 2026-10-24 had exactly these three)
    "sender_checks": [
        {"code": "auth-check", "params": {"check": "SPF", "result": "softfail"}},
        {"code": "auth-check", "params": {"check": "DKIM", "result": "none"}},
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


# as `GET /api/senders/{address}/callback-numbers` answers it
# (crates/webapi/src/handlers/callback_numbers.rs)
GIVEN_BEFORE = [
    {"number": "0120111222", "first_seen": 1754300000, "first_message_id": "<older@x>"},
    {"number": "81312345678", "first_seen": 1754400000, "first_message_id": HELD_MESSAGE},
]


def get(handler, messages):
    """The verdict route, the senders' numbers, and the armed thread; True
    when this answered."""
    path = handler.path.split("?")[0]
    numbers = re.match(r"^/api/senders/([^/]+)/callback-numbers$", path)
    if numbers:
        sender = unquote(numbers.group(1)).lower()
        given = []
        if ARMED[0] and sender == "spoofed@example.com":
            given = copy.deepcopy(GIVEN_BEFORE)
        handler._send({"sender": sender, "numbers": given})
        return True
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
