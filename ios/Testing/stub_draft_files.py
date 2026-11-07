"""Files kept with a draft, as `crates/webapi/src/handlers/draft_attachments.rs`
and `draft_send.rs` answer them.

`POST /api/mail/draft-attachments` takes one multipart file field and
answers the `DraftAttachment` a draft save names (`blob_id` is `U` and the
sha256 of the bytes). A file whose name starts with `refuse-` is answered
413 with the server's body, so a client can be shown to say the server's
reason. `GET /api/mail/draft-attachments/{blob_id}` serves the bytes.

A draft save without `attachments` keeps the draft's files and `[]`
clears them; `GET /api/mail/drafts` lists drafts with files only when
asked with `with_attachments=true`, and a send naming `draft_id` deletes
that draft. After `POST /debug/refuse-draft-save` every draft save is
refused with 500.

`GET /debug/draft-files` answers the uploads, the downloads, and each
draft's files.
"""

import hashlib
from email.parser import BytesParser
from urllib.parse import parse_qs, urlparse

LIMIT = 25 * 1024 * 1024

UPLOADS = []
BLOBS = {}
DOWNLOADS = []
REFUSE_SAVE = [False]


def _path(handler):
    return handler.path.split("?")[0]


def get(handler, drafts):
    path = _path(handler)
    if path == "/debug/draft-files":
        handler._send({
            "uploads": list(UPLOADS),
            "downloads": list(DOWNLOADS),
            "drafts": {str(k): d.get("attachments", []) for k, d in drafts.items()},
        })
        return True
    if path.startswith("/api/mail/draft-attachments/"):
        blob_id = path.rsplit("/", 1)[1]
        if blob_id not in BLOBS:
            handler._send({}, 404)
            return True
        DOWNLOADS.append(blob_id)
        query = parse_qs(urlparse(handler.path).query)
        handler._send_bytes(
            BLOBS[blob_id],
            query.get("type", ["application/octet-stream"])[0],
            query.get("name", ["attachment"])[0],
        )
        return True
    return False


def listed(handler, drafts):
    """The drafts `GET /api/mail/drafts` shows this caller, newest first."""
    query = parse_qs(urlparse(handler.path).query)
    with_files = query.get("with_attachments", ["false"])[0] == "true"
    shown = [d for d in drafts.values() if with_files or not d.get("attachments")]
    return sorted(shown, key=lambda d: -d["updated_at"])


def post(handler):
    path = _path(handler)
    if path == "/debug/refuse-draft-save":
        REFUSE_SAVE[0] = True
        handler._send({"ok": True})
        return True
    if path != "/api/mail/draft-attachments":
        return False
    length = int(handler.headers.get("Content-Length", "0"))
    raw = handler.rfile.read(length)
    msg = BytesParser().parsebytes(
        b"Content-Type: " + handler.headers.get("Content-Type", "").encode() +
        b"\r\n\r\n" + raw)
    part = next((p for p in msg.walk() if not p.is_multipart() and p.get_filename()), None)
    if part is None:
        handler._send({"error": "no file in the form"}, 400)
        return True
    name = part.get_filename()
    if name.startswith("refuse-"):
        handler._send({"error": "the message is larger than the 25 MiB limit",
                       "limit_bytes": LIMIT}, 413)
        return True
    payload = part.get_payload(decode=True) or b""
    blob_id = "U" + hashlib.sha256(payload).hexdigest()
    BLOBS[blob_id] = payload
    stored = {"blob_id": blob_id, "name": name,
              "content_type": part.get_content_type(), "size": len(payload)}
    UPLOADS.append(stored)
    handler._send(stored)
    return True


def refused_save(handler):
    if not REFUSE_SAVE[0]:
        return False
    handler._send({"error": "draft store unavailable"}, 500)
    return True


def kept(body, before):
    """The files a save leaves on the draft: absent keeps, a list replaces."""
    if "attachments" in body and body["attachments"] is not None:
        return body["attachments"]
    if before is None:
        return []
    return before.get("attachments", [])


def sent(record, drafts):
    """A send naming a draft deletes it, as the server does once it is queued."""
    draft_id = record.get("draft_id")
    if draft_id is None:
        return
    drafts.pop(int(draft_id), None)


def reset():
    UPLOADS.clear()
    BLOBS.clear()
    DOWNLOADS.clear()
    REFUSE_SAVE[0] = False
