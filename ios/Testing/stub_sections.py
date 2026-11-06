"""The sectioned Inbox and team rows, answered the way the server does.

`GET /api/conversations?folder=Inbox&section=…` narrows the Inbox
(`crates/mailbox/src/section.rs`). Each section answers its own rows, so a
client that sent no `section` (and got the whole Inbox) cannot pass a test
that looks for a section's row. `section=awaiting` with no folder is the
"Waiting for a reply" list, and `assigned_to_me=true` the "Assigned to me"
list (`crates/webapi/src/handlers/conversations.rs`).

- `reply`: a team conversation assigned to bob, so the row can name who
  answers it, and a plain one beside it.
- `awaiting`: one the reader is waiting on.
- `new`: two first contacts, one of them at an address in angle brackets,
  until `POST /api/senders/accept` names them; accepted senders leave New.

`GET /debug/accepted` reads back what was accepted, so a test can check
the request and not only the screen.
"""

import json
from urllib.parse import urlparse

import stub_team

BASE_TS = 1754500000

ACCEPTED = []


def _row(tid, subject, snippet, ts, participants, **extra):
    row = {
        "thread_id": tid, "subject": subject, "participants": participants,
        "message_count": 1, "unread_count": 0, "last_date": ts, "category": "inbox",
        "flagged": False, "snippet": snippet, "pinned": False, "archived": False,
        "importance_level": "normal", "importance_score": 0.0, "requires_action": False,
        "received_count": 1, "sent_count": 0, "assignee": "", "section": "",
    }
    row.update(extra)
    return row


def _rows(section):
    if section == "reply":
        return [
            _row("sec_team", "Customer asks about the refund", "who is taking this one",
                 BASE_TS + 30, ["customer@shop.test"], section="reply",
                 assignee="bob@golia.jp", shared_conv="1:7"),
            _row("sec_reply", "Can you confirm Thursday", "a yes or no is enough",
                 BASE_TS + 20, ["ann@x.test"], section="reply"),
        ]
    if section == "awaiting":
        return [_row("sec_wait", "Contract draft sent", "waiting on their legal team",
                     BASE_TS + 10, ["me@golia.jp", "legal@partner.test"], section="awaiting",
                     sent_count=1)]
    if section == "new":
        rows = [
            _row("sec_new1", "Hello from a first contact", "we have not met",
                 BASE_TS + 5, ["Newcomer <New@First.test>"], section="new"),
            _row("sec_new2", "Introduction", "a colleague suggested I write",
                 BASE_TS + 4, ["second@first.test"], section="new"),
        ]
        return [r for r in rows if _sender(r) not in ACCEPTED]
    return []


def _sender(row):
    shown = row["participants"][-1]
    if "<" in shown and ">" in shown:
        shown = shown[shown.index("<") + 1:shown.rindex(">")]
    return shown.strip().lower()


def answer(handler, query):
    """Answer a sectioned list, or the reader's assigned conversations;
    False when neither was asked for."""
    if query.get("assigned_to_me", [""])[0] == "true":
        handler._send(_assigned())
        return True
    section = query.get("section", [""])[0]
    if not section:
        return False
    handler._send(_rows(section))
    return True


def _assigned():
    """`assigned_to_me=true`: what a teammate handed the reader, across
    folders. One is a team conversation with its shared id, the other an
    unread one from the Junk folder, which an Inbox-only filter would drop."""
    return [
        _row("t41", "Invoice dispute from the reseller", "can you take this one",
             BASE_TS + 40, ["reseller@shop.test"], assignee="me@golia.jp",
             shared_conv="1:9", section="reply"),
        _row("t42", "Assigned: supplier onboarding", "the forms are attached",
             BASE_TS + 35, ["supplier@vendor.test"], assignee="me@golia.jp",
             unread_count=1, category="junk"),
    ]


def post(handler):
    path = urlparse(handler.path).path
    if path != "/api/senders/accept":
        return False
    length = int(handler.headers.get("Content-Length", "0"))
    body = {}
    if length:
        body = json.loads(handler.rfile.read(length))
    added = 0
    for address in body.get("addresses", []):
        a = address.strip().lower()
        if a and a not in ACCEPTED:
            ACCEPTED.append(a)
            added += 1
    handler._send({"accepted": added})
    return True


def get(handler):
    # the team conversation's thread, notes and the calendar live in stub_team
    if stub_team.get(handler):
        return True
    if urlparse(handler.path).path != "/debug/accepted":
        return False
    handler._send(list(ACCEPTED))
    return True


def reset():
    ACCEPTED.clear()

