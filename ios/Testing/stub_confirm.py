"""Recent password confirmation, as `crates/webapi/src/session/confirm.rs` answers it.

The operations below are refused with 403 `confirm_required` until the
session has confirmed its password with `POST /api/auth/confirm`. Any
password confirms except `stub_login.WRONG_PASSWORD`, which is refused the
way the server refuses one (401 `confirm_failed` with the tries left).
There is one session in this stub, so one flag; `/debug/reset` clears it.
"""

import json
import re

import stub_login

CONFIRMED = [False]
FAILURES = [0]

# the ones the phone clients send; the server guards more
GUARDED = [
    ("POST", r"^/api/admin/aliases$"),
    ("DELETE", r"^/api/admin/aliases/\d+$"),
    ("POST", r"^/api/admin/email-groups/\d+/members$"),
    ("DELETE", r"^/api/admin/email-groups/\d+/members/.+$"),
    ("POST", r"^/api/agent/keys$"),
]


def refused(handler, method):
    """Answer 403 and return True when `method` on this path needs a
    confirmation the session does not have."""
    if CONFIRMED[0]:
        return False
    path = handler.path.split("?")[0]
    if not any(m == method and re.match(p, path) for m, p in GUARDED):
        return False
    handler._send({"error": "confirm_required"}, status=403)
    return True


def answer(handler):
    length = int(handler.headers.get("Content-Length", "0"))
    body = json.loads(handler.rfile.read(length)) if length else {}
    if not isinstance(body.get("password"), str):
        handler._send({"error": "missing field `password`"}, status=422)
        return
    if body["password"] == stub_login.WRONG_PASSWORD:
        FAILURES[0] += 1
        handler._send({"error": "confirm_failed", "remaining": 5 - FAILURES[0]}, status=401)
        return
    CONFIRMED[0] = True
    FAILURES[0] = 0
    handler.send_response(204)
    handler.send_header("Content-Length", "0")
    handler.send_header("Connection", "close")
    handler.end_headers()


def reset():
    CONFIRMED[0] = False
    FAILURES[0] = 0
