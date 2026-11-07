"""Files kept with a draft, as `crates/webapi/src/handlers/draft_attachments.rs`
answers them.

`POST /api/mail/draft-attachments` takes a multipart form with one file and
answers the `DraftAttachment` a draft save names (`blob_id` is `U` and the
SHA-256 of the bytes). A file whose name contains `too-large` is refused
with 413 and the server's body, so a client can be checked to show the
server's reason. `GET /api/mail/draft-attachments/{blob_id}` serves a stored
file and is 404 for anything else; `/debug/draft-file-gets` lists the blob
ids asked for.

A send that names `draft_id` gets the draft's files ahead of the form's and
deletes the draft, as `draft_send.rs` does; a draft that is gone is 404.
`/debug/reset` clears everything.
"""

import hashlib
import re
from email.parser import BytesParser
from urllib.parse import unquote

FILES = {}
GETS = []


def post(handler):
    if handler.path.split("?")[0] != "/api/mail/draft-attachments":
        return False
    length = int(handler.headers.get("Content-Length", "0"))
    raw = handler.rfile.read(length)
    msg = BytesParser().parsebytes(
        b"Content-Type: " + handler.headers.get("Content-Type", "").encode() +
        b"\r\n\r\n" + raw)
    part = next((p for p in msg.walk() if not p.is_multipart()), None)
    if part is None:
        handler._send({"error": "no file in the form"}, status=400)
        return True
    name = part.get_filename() or "attachment"
    data = part.get_payload(decode=True) or b""
    if "too-large" in name:
        handler._send({"error": "the message is larger than the 25 MiB limit",
                       "limit_bytes": 26214400}, status=413)
        return True
    blob_id = "U" + hashlib.sha256(data).hexdigest()
    FILES[blob_id] = data
    handler._send({"blob_id": blob_id, "name": name,
                   "content_type": part.get_content_type(), "size": len(data)})
    return True


def get(handler):
    path = handler.path.split("?")[0]
    if path == "/debug/draft-file-gets":
        handler._send({"gets": list(GETS)})
        return True
    m = re.match(r"^/api/mail/draft-attachments/([^/]+)$", path)
    if not m:
        return False
    blob_id = unquote(m.group(1))
    GETS.append(blob_id)
    data = FILES.get(blob_id)
    if data is None:
        handler._send({}, status=404)
        return True
    handler.send_response(200)
    handler.send_header("Content-Type", "application/octet-stream")
    handler.send_header("Content-Disposition", "attachment")
    handler.send_header("Content-Length", str(len(data)))
    handler.end_headers()
    handler.wfile.write(data)
    return True


def attach(handler, record, drafts):
    """Put the files of the draft the send names into `record` as
    `draft_attachments` and delete the draft. False when it answered."""
    raw = record.get("draft_id")
    if raw is None:
        return True
    draft = drafts.pop(int(raw), None)
    if draft is None:
        handler._send({"error": "the draft is gone; it may have been sent already"},
                      status=404)
        return False
    record["draft_attachments"] = [
        {"name": a["name"], "bytes": len(FILES.get(a["blob_id"], b""))}
        for a in draft.get("attachments", [])
    ]
    return True


def reset():
    FILES.clear()
    GETS.clear()
