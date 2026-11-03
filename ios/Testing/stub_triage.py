"""The lists and verbs filing mail uses beyond the inbox's, as webapi answers them.

Lists (`GET /api/conversations` with `pinned`, `snoozed`, `trashed`,
`label` or `folder=all`, `GET /api/quarantine`, `GET /api/labels`) each
answer their own rows, so a client that asked for the wrong list shows the
wrong row. The Trash, Review and a label are worked out from the verbs the
stub has seen, so a conversation moved by one request is where the next
request looks: put in the Trash, it is listed there; restored, it is not.

Verbs (`pin`, `unpin`, `/snooze`, `/forever`, release) are recorded in the
same `verbs` list as the rest, as `<verb> <thread id>`. Deleting for good
needs a recent password confirmation, as on the server (`stub_confirm`).
"""

import re
import time
from urllib.parse import parse_qs, urlparse

# Two labels, one of them named in Japanese: a label name is the reader's
# own text and the one place a list title is not from the app's strings.
LABELS = [
    {"id": "1", "name": "Work", "color": "blue", "created_at": 1754400000},
    {"id": "2", "name": "旅行", "color": "green", "created_at": 1754400001},
]


def _row(tid, subject, ts, **extra):
    row = {
        "thread_id": tid, "subject": subject, "participants": ["someone@example.com"],
        "message_count": 1, "unread_count": 0, "last_date": ts, "category": "inbox",
        "flagged": False, "snippet": "", "pinned": False, "archived": False,
        "importance_level": "normal", "importance_score": 0.0, "requires_action": False,
        "received_count": 1, "sent_count": 0,
    }
    row.update(extra)
    return row


def _moved(verbs, into, out_of, base):
    """The ids in a place after replaying the verbs that move into and out of it."""
    ids = list(base)
    for v in verbs:
        verb, _, tid = v.partition(" ")
        if verb == into and tid not in ids:
            ids.append(tid)
        if verb == out_of and tid in ids:
            ids.remove(tid)
    return ids


def get(handler, verbs):
    """Answer the lists above; True when this was one."""
    url = urlparse(handler.path)
    path, q = url.path, parse_qs(url.query)
    flag = lambda name: q.get(name, [""])[0] in ("true", "1")
    if path == "/api/labels":
        handler._send(LABELS)
        return True
    if path == "/api/quarantine":
        held = _moved(verbs, "", "release", ["held1"])
        rows = [_row(t, "Your account will be suspended", 1754300000) for t in held]
        handler._send({"items": rows, "total": len(rows)})
        return True
    if path != "/api/conversations":
        return False
    if flag("trashed"):
        ids = _moved(verbs, "delete", "untrash", ["trash1"])
        handler._send([_row(t, "In the Trash: " + t, 1754310000, trashed_at=1754390000) for t in ids])
        return True
    if flag("pinned"):
        # pinned and unread, so the menu has to offer Mark as read
        handler._send([_row("pin1", "Pinned thread", 1754320000, pinned=True, unread_count=1)])
        return True
    if flag("snoozed"):
        later = int(time.time()) + 86400
        handler._send([_row("snz1", "Snoozed thread", 1754330000, snoozed_until=later)])
        return True
    label = q.get("label", [None])[0]
    if label is not None:
        ids = _moved(verbs, "label", "unlabel", ["lab1"] if label == "1" else [])
        handler._send([_row(t, "Labelled: " + t, 1754340000, labels=[label]) for t in ids])
        return True
    if q.get("folder", [""])[0] == "all":
        handler._send([_row("all1", "In All mail", 1754350000),
                       _row("all2", "Archived, still in All mail", 1754340000, archived=True)])
        return True
    return False


def post(handler, verbs):
    """Pin, unpin and release; True when this was one."""
    path = handler.path.split("?")[0]
    m = re.match(r"^/api/conversations/([\w%.@-]+)/(pin|unpin)$", path)
    if m:
        verbs.append(f"{m.group(2)} {m.group(1)}")
        _no_content(handler)
        return True
    m = re.match(r"^/api/quarantine/([\w%.@-]+)/release$", path)
    if m:
        verbs.append(f"release {m.group(1)}")
        _no_content(handler)
        return True
    return False


def put(handler, verbs):
    m = re.match(r"^/api/conversations/([\w%.@-]+)/snooze$", handler.path.split("?")[0])
    if not m:
        return False
    verbs.append(f"snooze {m.group(1)}")
    _no_content(handler)
    return True


def delete(handler, verbs):
    m = re.match(r"^/api/conversations/([\w%.@-]+)/(snooze|forever)$", handler.path.split("?")[0])
    if not m:
        return False
    name = {"snooze": "unsnooze", "forever": "forever"}[m.group(2)]
    verbs.append(f"{name} {m.group(1)}")
    _no_content(handler)
    return True


def _no_content(handler):
    handler.send_response(204)
    handler.send_header("Content-Length", "0")
    handler.end_headers()
