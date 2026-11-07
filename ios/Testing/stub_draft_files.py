"""Files kept with a draft, as `crates/webapi/src/handlers/draft_attachments.rs`
and `draft_send.rs` answer them.

`POST /api/mail/draft-attachments` takes one multipart file field and
answers the `DraftAttachment` a draft save names (`blob_id` is `U` and the
sha256 of the bytes). A file whose name contains `too-large` or starts
with `refuse-` is answered 413 with the server's body, so a client can be
shown to say the server's reason. `GET /api/mail/draft-attachments/{blob_id}`
serves the bytes and is 404 for a blob it does not hold.

A draft save without `attachments` keeps the draft's files and a list,
`[]` included, replaces them; `GET /api/mail/drafts` lists drafts with
files only when asked with `with_attachments=true`. A send naming
`draft_id` records the draft's files under `draft_attachments` (apart from
the form's own) and deletes that draft; a draft that is gone is 404.
After `POST /debug/refuse-draft-save` every draft save is refused with 500.

`GET /debug/draft-files` answers the uploads, the downloads that were
served, and each draft's files; `GET /debug/draft-file-gets` lists every
blob id asked for, served or not. `/debug/reset` clears everything.
"""

import hashlib
from email.parser import BytesParser
from urllib.parse import parse_qs, unquote, urlparse

LIMIT = 25 * 1024 * 1024

UPLOADS = []
BLOBS = {}
DOWNLOADS = []
GETS = []
REFUSE_SAVE = [False]


def _path(handler):
    return handler.path.split("?")[0]


def _refused(name):
    return "too-large" in name or name.startswith("refuse-")


def get(handler, drafts):
    path = _path(handler)
    if path == "/debug/draft-files":
        handler._send({
            "uploads": list(UPLOADS),
            "downloads": list(DOWNLOADS),
            "drafts": {str(k): d.get("attachments", []) for k, d in drafts.items()},
        })
        return True
    if path == "/debug/draft-file-gets":
        handler._send({"gets": list(GETS)})
        return True
    if path.startswith("/api/mail/draft-attachments/"):
        blob_id = unquote(path.rsplit("/", 1)[1])
        GETS.append(blob_id)
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
    leaves = [p for p in msg.walk() if not p.is_multipart()]
    part = next((p for p in leaves if p.get_filename()), None)
    if part is None and leaves:
        part = leaves[0]
    if part is None:
        handler._send({"error": "no file in the form"}, 400)
        return True
    name = part.get_filename() or "attachment"
    if _refused(name):
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
    """True when the save switch is on and the save was answered 500."""
    if not REFUSE_SAVE[0]:
        return False
    handler._send({"error": "draft store unavailable"}, 500)
    return True


def kept(body, before):
    """The files a save leaves on the draft: absent keeps, a list replaces."""
    if body.get("attachments") is not None:
        return body["attachments"]
    if before is None:
        return []
    return before.get("attachments", [])


def sent(handler, record, drafts):
    """A send naming a draft carries the draft's files and deletes it, as
    the server does once it is queued. The files are recorded apart from
    the form's, under `draft_attachments`, so a test can tell a file the
    server attached from one the client uploaded again. False when the
    draft is gone and the send was answered 404; the caller records
    nothing then."""
    draft_id = record.get("draft_id")
    if draft_id is None:
        return True
    draft = drafts.pop(int(draft_id), None)
    if draft is None:
        handler._send({"error": "the draft is gone; it may have been sent already"}, 404)
        return False
    record["draft_attachments"] = [
        {"name": a["name"], "filename": a["name"],
         "content_type": a.get("content_type", "application/octet-stream"),
         "bytes": len(BLOBS.get(a["blob_id"], b""))}
        for a in draft.get("attachments", [])
    ]
    return True


def reset():
    UPLOADS.clear()
    BLOBS.clear()
    DOWNLOADS.clear()
    GETS.clear()
    REFUSE_SAVE[0] = False
