"""Blocking a sender from a message, as `block_sender.rs` answers it.

`POST /api/conversations/{id}/block-sender {"address"}` puts the address
on the blacklist, takes it off the whitelist, records `block-sender <id>`
in the verbs and answers `{"address", "was_allowed"}`; the reader's own
address is refused with 422 and nothing is written.
`POST /api/conversations/{id}/unblock-sender {"address", "allow"}` takes
it off the blacklist, back on the whitelist when `allow`, and records
`unblock-sender <id>`; it answers 204.

The plain list writes keep the same rule, an address on one list at most,
so a client that blocks an allowed address sees it leave the allowed list.
"""

import json
import re
from urllib.parse import unquote

OWN = "me@golia.jp"

_VERB = re.compile(r"^/api/conversations/(.+)/(block-sender|unblock-sender)$")


def post(handler, lists, verbs):
    """Answer a block or unblock. False for any other request."""
    m = _VERB.match(handler.path.split("?")[0])
    if not m:
        return False
    tid, verb = unquote(m.group(1)), m.group(2)
    length = int(handler.headers.get("Content-Length", "0"))
    body = json.loads(handler.rfile.read(length)) if length else {}
    address = str(body.get("address", "")).strip().lower()
    if not address:
        handler._send({"error": "missing field `address`"}, status=422)
        return True
    if verb == "block-sender":
        if address == OWN:
            handler._send({"error": "own address"}, status=422)
            return True
        was_allowed = address in lists["whitelist"]
        put(lists, address, "blacklist")
        verbs.append(f"block-sender {tid}")
        handler._send({"address": address, "was_allowed": was_allowed})
        return True
    lists["blacklist"][:] = [a for a in lists["blacklist"] if a != address]
    if body.get("allow"):
        put(lists, address, "whitelist")
    verbs.append(f"unblock-sender {tid}")
    handler.send_response(204)
    handler.send_header("Content-Length", "0")
    handler.end_headers()
    return True


def put(lists, address, kind):
    """Onto `kind`, and off the other list."""
    other = "whitelist"
    if kind == "whitelist":
        other = "blacklist"
    lists[other][:] = [a for a in lists[other] if a != address]
    if address not in lists[kind]:
        lists[kind].append(address)
