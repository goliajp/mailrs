"""A team conversation's notes and overview, and the reader's calendar.

The team conversation is `sec_team` (shared_conv "1:7"), the row the
sectioned Inbox lists under To reply.

- `GET /api/conversations/sec_team`: one customer message.
- `GET /api/shared/1/7`: assigned to bob, carol has a reply in progress.
- `GET /api/shared/1/7/notes`: two notes, the second one edited
  (`handlers/shared_inbox/notes.rs`).
- `GET /api/calendar/events?start=&end=`: the week the client asks for,
  placed from `start` so the events always land in the week on screen:
  - a timed event on the Monday;
  - one with no end, which lasts an hour as the server reads it;
  - an all-day event on Wednesday and Thursday, sent as dates with the end
    exclusive, which must not slide to another day in any zone.
"""

import datetime
from urllib.parse import parse_qs, urlparse

BASE_TS = 1754500000

TEAM_MESSAGE = {
    "uid": 31, "sender": "Customer <customer@shop.test>", "sender_trust": "verified",
    "recipients": "support@golia.jp", "subject": "Customer asks about the refund",
    "flags": 0, "internal_date": BASE_TS + 30, "message_id": "<team31@shop.test>",
    "text_body": "Who is taking this one? I would like my refund.",
    "html_body": "<p>Who is taking this one? I would like my refund.</p>",
    "attachments": [], "category": "inbox", "risk_score": 0, "risk_reason": "",
    "summary": "", "people": {}, "dates": {}, "amounts": {}, "action_items": [],
    "ai_analyzed": False, "importance_level": "normal", "importance_score": 0.1,
    "is_bulk_sender": False, "has_tracking_pixel": False, "requires_action": False,
    "sender_intent": "", "invite_method": "",
}

OVERVIEW = {
    "gid": 1, "conv": 7, "assignee": "bob@golia.jp",
    "assignments": [{"at": BASE_TS, "by": "carol@golia.jp", "to": "bob@golia.jp"}],
    "draft": {"author": "carol@golia.jp", "draft_id": 3, "updated_at": BASE_TS + 40},
    "handoffs": [], "members": ["me@golia.jp", "bob@golia.jp", "carol@golia.jp"],
    "present": [], "reply": {"at": 0, "by": "", "seq": 0},
}

NOTES = [
    {"id": 1, "at": BASE_TS + 31, "by": "bob@golia.jp", "edited_at": None,
     "text": "Refund approved, waiting on finance"},
    {"id": 2, "at": BASE_TS + 32, "by": "carol@golia.jp", "edited_at": BASE_TS + 33,
     "text": "Customer prefers a bank transfer"},
]


def _monday(start):
    """The local Monday the client's window starts on, whatever its zone."""
    instant = datetime.datetime.fromisoformat(start.replace("Z", "+00:00"))
    # every zone is within 14 hours of UTC; noon of the UTC instant lands
    # on the reader's Monday for offsets between -12 and +14 hours
    return (instant + datetime.timedelta(hours=12)).date()


def _events(start):
    monday = _monday(start)
    day = datetime.timedelta(days=1)
    stamp = "%Y-%m-%dT%H:%M:%SZ"
    timed = datetime.datetime.combine(monday, datetime.time(9, 0)) - datetime.timedelta(hours=9)
    open_ended = timed + 2 * day
    return [
        {"uid": "ev-standup", "summary": "Weekly standup",
         "dtstart": timed.strftime(stamp),
         "dtend": (timed + datetime.timedelta(minutes=30)).strftime(stamp),
         "organizer": "bob@golia.jp", "source": None, "status": "CONFIRMED"},
        {"uid": "ev-open", "summary": "Call with the bank",
         "dtstart": open_ended.strftime(stamp), "dtend": None,
         "organizer": None, "source": None, "status": None},
        {"uid": "ev-offsite", "summary": "Team offsite",
         "dtstart": (monday + 2 * day).isoformat(), "dtend": (monday + 4 * day).isoformat(),
         "organizer": None, "source": None, "status": "CONFIRMED"},
    ]


def get(handler):
    path = urlparse(handler.path).path
    if path == "/api/conversations/sec_team":
        handler._send([TEAM_MESSAGE])
        return True
    if path == "/api/shared/1/7":
        handler._send(OVERVIEW)
        return True
    if path == "/api/shared/1/7/notes":
        handler._send(NOTES)
        return True
    if path == "/api/calendar/events":
        query = parse_qs(urlparse(handler.path).query)
        start = query.get("start", [""])[0]
        if not start or not query.get("end"):
            handler.send_response(400)
            handler.end_headers()
            return True
        handler._send(_events(start))
        return True
    return False
