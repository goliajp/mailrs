"""A reader's phones on UnifiedPush, answered the way `handlers/unified_push.rs` does.

- `GET /api/push/unifiedpush`: `{enabled, vapid_public_key, devices}`; a
  device is `{id, name, host, created_at}`, never its endpoint. One phone
  is already registered, an old one on another distributor, so a list is
  never only the device under test.
- `POST /api/push/unifiedpush {endpoint, keys: {p256dh, auth}, name}`: 400
  for an endpoint that is not https or a key that is not there, as the
  server refuses; the same endpoint keeps its id. Answers `{id}`.
- `DELETE /api/push/unifiedpush/{id}`: 204 whether or not it was there.

`GET /debug/unifiedpush` reads back every write, endpoints and keys
included, so a test checks what the phone sent and not only the screen.
`POST /debug/unifiedpush-off` makes the server answer as one with no VAPID
key: `enabled: false`, and registration refused with 409.
`POST /debug/unifiedpush-evict/{id}` forgets a device the way the
per-account cap does, without the phone being told and without counting as
one of its writes. `GET /debug/unifiedpush-reads` is how many times the list
was read, so a test can tell "checked and found it there" from "never asked".
"""

import copy
import hashlib
import json
from urllib.parse import unquote, urlparse

BASE_TS = 1754500000
# a real P-256 point, base64url: the connector checks the shape of the key
VAPID = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"

WRITES = []
READS = {"list": 0}
FRESH = {
    "enabled": True,
    "devices": {
        "d-old": {"endpoint": "https://push.example.net/upOLDPHONE", "name": "Old Moto",
                  "created_at": BASE_TS - 86400 * 40},
    },
}
STATE = copy.deepcopy(FRESH)


def reset():
    WRITES.clear()
    READS["list"] = 0
    STATE.clear()
    STATE.update(copy.deepcopy(FRESH))


def _body(handler):
    length = int(handler.headers.get("Content-Length", "0"))
    if not length:
        return {}
    return json.loads(handler.rfile.read(length))


def _empty(handler, status=204):
    handler.send_response(status)
    handler.send_header("Content-Length", "0")
    handler.end_headers()


def _listed():
    rows = [{"id": i, "name": d["name"], "host": urlparse(d["endpoint"]).hostname or "",
             "created_at": d["created_at"]} for i, d in STATE["devices"].items()]
    return sorted(rows, key=lambda r: (r["created_at"], r["id"]))


def get(handler):
    path = urlparse(handler.path).path
    if path == "/api/push/unifiedpush":
        READS["list"] += 1
        answer = {"enabled": STATE["enabled"], "devices": _listed()}
        if STATE["enabled"]:
            answer["vapid_public_key"] = VAPID
        handler._send(answer)
        return True
    if path == "/debug/unifiedpush":
        handler._send(WRITES)
        return True
    if path == "/debug/unifiedpush-reads":
        handler._send(READS)
        return True
    return False


def post(handler):
    path = urlparse(handler.path).path
    if path == "/debug/unifiedpush-off":
        STATE["enabled"] = False
        handler._send({"ok": True})
        return True
    evict = "/debug/unifiedpush-evict/"
    if path.startswith(evict):
        handler._send({"evicted": STATE["devices"].pop(unquote(path[len(evict):]), None) is not None})
        return True
    if path != "/api/push/unifiedpush":
        return False
    body = _body(handler)
    WRITES.append({"method": "POST", "path": path, "body": body})
    if not STATE["enabled"]:
        _empty(handler, 409)
        return True
    keys = body.get("keys") or {}
    endpoint = body.get("endpoint", "")
    if not endpoint.startswith("https://") or not keys.get("p256dh") or not keys.get("auth"):
        _empty(handler, 400)
        return True
    device_id = hashlib.sha256(endpoint.encode()).hexdigest()
    STATE["devices"][device_id] = {"endpoint": endpoint, "name": body.get("name", ""),
                                   "created_at": BASE_TS + len(WRITES)}
    handler._send({"id": device_id})
    return True


def delete(handler):
    path = urlparse(handler.path).path
    prefix = "/api/push/unifiedpush/"
    if not path.startswith(prefix):
        return False
    device_id = unquote(path[len(prefix):])
    WRITES.append({"method": "DELETE", "path": path, "body": None})
    STATE["devices"].pop(device_id, None)
    _empty(handler)
    return True
