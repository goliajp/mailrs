"""`POST /api/auth/login`, as `crates/webapi/src/handlers/auth/session.rs` answers it.

Any password signs any address in, except two cases: `WRONG_PASSWORD`
is refused the way the server refuses a bad password (401, no body), and
`TOTP_ADDRESS` stands for an account with two-factor sign-in turned on,
so a client's code prompt has something to talk to.
"""

import json

TOTP_ADDRESS = "totp@golia.jp"
TOTP_CODE = "123456"
WRONG_PASSWORD = "wrong-password"


def answer(handler):
    length = int(handler.headers.get("Content-Length", "0"))
    body = json.loads(handler.rfile.read(length)) if length else {}
    # The server's own body shape: a client that sends another
    # field name gets the 422 the server would give it, not a token.
    if not isinstance(body.get("address"), str) or not isinstance(body.get("password"), str):
        handler._send({"error": "missing field `address` or `password`"}, status=422)
        return
    if body["password"] == WRONG_PASSWORD:
        handler.send_response(401)
        handler.send_header("Content-Length", "0")
        handler.send_header("Connection", "close")
        handler.end_headers()
        return
    if body["address"] != TOTP_ADDRESS:
        handler._send({"address": "me@golia.jp", "display_name": "Me",
                       "permissions": [], "token": "stub-token"})
        return
    code = body.get("totp_code")
    # a missing code is a 200 with another body, not an error
    if code is None:
        handler._send({"requires_totp": True})
        return
    if code != TOTP_CODE:
        handler._send({"error": "invalid TOTP code"}, status=401)
        return
    handler._send({"address": TOTP_ADDRESS, "display_name": "Two Factor",
                   "permissions": [], "token": "stub-token"})
