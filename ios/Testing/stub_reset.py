"""Cleared passwords and reset codes, as the server answers them.

`POST /api/auth/login` for `CLEARED_ADDRESS` is refused the way the server
refuses an account whose password an administrator cleared: 401
`password_cleared`, with whom to ask. `POST /api/auth/password-reset`
takes `GOOD_CODE` (typed any way: dashes, spaces, lower case) and a new
password of at least 12 characters; any other code is `invalid_reset_code`.
`GET /debug/reset-state` says whether a reset went through. `/debug/reset`
clears it.
"""

import json

CLEARED_ADDRESS = "cleared@golia.jp"
GOOD_CODE = "GOODCODE23"
CONTACT = {"address": "lihao@golia.jp", "name": "Li Hao"}

DONE = [False]


def _body(handler):
    length = int(handler.headers.get("Content-Length", "0"))
    return json.loads(handler.rfile.read(length)) if length else {}


def login(handler, body):
    """Answer a sign-in to the cleared account; True when this was one."""
    if body.get("address") != CLEARED_ADDRESS:
        return False
    handler._send({"contact": CONTACT, "error": "password_cleared"}, status=401)
    return True


def handle(handler):
    """Handle `POST /api/auth/password-reset`; True when this was it."""
    if handler.path.split("?")[0] != "/api/auth/password-reset":
        return False
    _answer(handler, _body(handler))
    return True


def _answer(handler, body):
    fields = ("address", "code", "new_password")
    if not all(isinstance(body.get(f), str) for f in fields):
        handler._send({"error": "missing field"}, status=422)
        return
    typed = "".join(c for c in body["code"] if not c.isspace() and c != "-").upper()
    if body["address"] != CLEARED_ADDRESS or typed != GOOD_CODE:
        handler._send({"error": "invalid_reset_code"}, status=400)
        return
    if len(body["new_password"]) < 12:
        handler._send({"error": "Use at least 12 characters"}, status=400)
        return
    DONE[0] = True
    handler.send_response(204)
    handler.send_header("Content-Length", "0")
    handler.send_header("Connection", "close")
    handler.end_headers()


def state(handler):
    """Handle `GET /debug/reset-state`; True when this was it."""
    if handler.path.split("?")[0] != "/debug/reset-state":
        return False
    handler._send({"done": DONE[0]})
    return True


def reset():
    DONE[0] = False
