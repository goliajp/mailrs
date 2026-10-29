"""The server restarting during a deploy, as webapi answers it then.

`POST /debug/restarting/<n>?path=<path>` makes the next `n` requests to
`path` answer 503 with `Retry-After: 1`, the way webapi does while the core
replays its log. Naming the path keeps the refusal off the background
requests a client makes meanwhile, so a test is about the request it means.
A client should repeat a read once after that and never repeat a write.
`/debug/reset` clears it.
"""

import re
from urllib.parse import parse_qs, urlparse

REMAINING = [0]
PATH = [""]
# answers given to the armed path after its refusals: what shows that a
# client came back, which the screen cannot (it may be drawn from a cache)
SERVED_AFTER = [0]


def arm(handler):
    """Handle `POST /debug/restarting/<n>?path=...`; True when this was it."""
    m = re.match(r"^/debug/restarting/(\d+)$", handler.path.split("?")[0])
    if not m:
        return False
    REMAINING[0] = int(m.group(1))
    PATH[0] = parse_qs(urlparse(handler.path).query).get("path", [""])[0]
    SERVED_AFTER[0] = 0
    handler._send({"ok": True})
    return True


def state(handler):
    """Handle `GET /debug/restart-state`; True when this was it."""
    if handler.path.split("?")[0] != "/debug/restart-state":
        return False
    handler._send({"remaining": REMAINING[0], "served_after": SERVED_AFTER[0]})
    return True


def refused(handler):
    """Answer 503 and return True for an armed path."""
    if handler.path.split("?")[0] != PATH[0] or not PATH[0]:
        return False
    if REMAINING[0] <= 0:
        SERVED_AFTER[0] += 1
        return False
    REMAINING[0] -= 1
    body = b'{"error":"core unavailable"}'
    handler.send_response(503)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Retry-After", "1")
    handler.send_header("Content-Length", str(len(body)))
    handler.send_header("Connection", "close")
    handler.end_headers()
    handler.wfile.write(body)
    return True


def reset():
    REMAINING[0] = 0
    PATH[0] = ""
    SERVED_AFTER[0] = 0
