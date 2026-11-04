"""Composer extras: templates, and undoing a send the server still holds.

`GET /api/mail/templates` answers as `crates/webapi/src/handlers/prefs.rs`
does (a bare array of `TemplateWire`), with one template that carries a
subject and one that has only a body — applying the second must not
touch the subject. `DELETE /api/mail/pending/{message_id}` answers 204
and records the id; after `POST /debug/send-late` it answers 409, the
server's "already on its way". `GET /debug/pending-cancels` lists the ids
asked for. `/debug/reset` clears both.
"""

import json
import re
from urllib.parse import unquote

TEMPLATES = [
    {"id": 1, "name": "Thanks", "subject": "Thank you",
     "text_body": "Thank you for your message.", "html_body": "", "category": "general"},
    {"id": 2, "name": "Signature only", "subject": "",
     "text_body": "Best regards", "html_body": "", "category": "general"},
]

CANCELS = []
LATE = [False]


def get(handler):
    path = handler.path.split("?")[0]
    if path == "/api/mail/templates":
        handler._send(TEMPLATES)
        return True
    if path == "/debug/pending-cancels":
        handler._send({"cancels": list(CANCELS), "late": LATE[0]})
        return True
    return False


def post(handler):
    if handler.path.split("?")[0] == "/debug/send-late":
        LATE[0] = True
        handler._send({"ok": True})
        return True
    return False


def delete(handler):
    m = re.match(r"^/api/mail/pending/(.+)$", handler.path.split("?")[0])
    if not m:
        return False
    CANCELS.append(unquote(m.group(1)))
    if LATE[0]:
        handler._send({"error": "already sent"}, status=409)
        return True
    handler.send_response(204)
    handler.send_header("Content-Length", "0")
    handler.end_headers()
    return True


def save_draft(handler, drafts, counter, posts):
    """`POST /api/mail/drafts`, as `SaveDraftRequest`: an `id` upserts,
    none allocates one. `from` and `scheduled_at` are kept, as the server
    keeps them, so a resumed draft can be checked for both."""
    length = int(handler.headers.get("Content-Length", "0"))
    body = {}
    if length:
        body = json.loads(handler.rfile.read(length))
    posts.append(body.get("id"))
    draft_id = body.get("id")
    if draft_id is None:
        counter[0] += 1
        draft_id = counter[0]
    now = 1754400000 + draft_id
    drafts[draft_id] = {
        "id": draft_id, "to": body.get("to", ""), "cc": body.get("cc", ""),
        "bcc": body.get("bcc", ""), "subject": body.get("subject", ""),
        "body": body.get("body", ""),
        "reply_to_thread_id": body.get("reply_to_thread_id"),
        "from": body.get("from", ""), "scheduled_at": body.get("scheduled_at"),
        "created_at": now, "updated_at": now,
    }
    handler._send({"id": draft_id})


def reset():
    CANCELS.clear()
    LATE[0] = False
