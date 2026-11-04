"""The reader's own settings, answered the way the server does.

- `GET/PUT /api/notify`: push mode, quiet hours (minutes after local
  midnight, both or neither, not equal) and the offset they are read in
  (`handlers/notify.rs`). A bad window is a 400, as there.
- `GET /api/notify/overrides`; `PUT /api/notify/senders/{a}` (`always`,
  `never`, null) and `/api/notify/threads/{t}` (`follow`, `mute`, null).
- `GET /api/auth/sessions` (this one and an old laptop);
  `DELETE /api/auth/sessions/{id}`, 404 for one that is not there;
  `POST /api/auth/sessions/end-others` answers `{ended}`.
- `GET /api/auth/app-passwords`, one never used; `DELETE` one, 404 if gone.
- `GET/PUT /api/settings/vacation`: refused with `invalid_body` when
  turned on with no text, as `families/vacation.rs` refuses; each save
  raises `version`. The reply here has text, so it can be turned on.
- `GET/POST /api/masked-emails`, camelCase as `families/masked_email`.
- `POST /api/senders/reclassify {sender, bucket}` answers `{moved}`.

`GET /debug/settings` reads back every write, so a test checks the request
and not only the screen.
"""

import copy
import json
import re
from urllib.parse import unquote, urlparse

BASE_TS = 1754500000

WRITES = []

FRESH = {
    "notify": {"mode": "default", "hide_content": False, "utc_offset_minutes": 0},
    "overrides": {"senders": [["news@shop.test", "never"]], "threads": [["t1", "mute"]]},
    "sessions": [
        {"id": "s-this", "created_at": BASE_TS, "last_seen": BASE_TS + 600, "ip": "203.0.113.4",
         "user_agent": "Mailrs iPhone", "current": True},
        {"id": "s-laptop", "created_at": BASE_TS - 86400 * 30, "last_seen": BASE_TS - 86400 * 9,
         "ip": "198.51.100.23", "user_agent": "Firefox on Linux", "current": False},
    ],
    "app_passwords": [
        {"id": "ap-tb", "name": "Thunderbird", "created_at": BASE_TS - 86400 * 90, "last_used_at": BASE_TS},
        {"id": "ap-old", "name": "Old phone", "created_at": BASE_TS - 86400 * 400, "last_used_at": None},
    ],
    "vacation": {"enabled": False, "subject": "Away", "body": "Back on Monday.", "start": None,
                 "end": None, "days": 7, "version": 2},
    "masks": [
        {"id": "m1", "email": "quiet.otter@golia.jp", "owner": "me@golia.jp", "state": "enabled",
         "forDomain": "shop.test", "description": "Shop account", "url": None,
         "createdAt": BASE_TS - 86400, "createdBy": "web", "lastMessageAt": BASE_TS},
    ],
}
STATE = copy.deepcopy(FRESH)


def reset():
    WRITES.clear()
    STATE.clear()
    STATE.update(copy.deepcopy(FRESH))


def _body(handler):
    length = int(handler.headers.get("Content-Length", "0"))
    if not length:
        return {}
    return json.loads(handler.rfile.read(length))


def _empty(handler, status=204):
    handler.send_response(status)
    handler.end_headers()


def _write(method, path, body):
    WRITES.append({"method": method, "path": path, "body": body})


def get(handler):
    path = urlparse(handler.path).path
    answers = {
        "/api/notify": lambda: STATE["notify"],
        "/api/notify/overrides": lambda: STATE["overrides"],
        "/api/auth/sessions": lambda: STATE["sessions"],
        "/api/auth/app-passwords": lambda: STATE["app_passwords"],
        "/api/settings/vacation": lambda: STATE["vacation"],
        "/api/masked-emails": lambda: STATE["masks"],
        "/debug/settings": lambda: WRITES,
    }
    answer = answers.get(path)
    if answer is None:
        return False
    handler._send(answer())
    return True


def _window_ok(p):
    start, end = p.get("quiet_start"), p.get("quiet_end")
    if start is None and end is None:
        return True
    if start is None or end is None:
        return False
    return 0 <= start < 1440 and 0 <= end < 1440 and start != end


def _override(kind, key, value):
    rows = [r for r in STATE["overrides"][kind] if r[0] != key]
    if value is not None:
        rows.append([key, value])
    STATE["overrides"][kind] = rows


def put(handler):
    path = urlparse(handler.path).path
    if path == "/api/notify":
        body = _body(handler)
        _write("PUT", path, body)
        if body.get("mode") not in ("default", "all", "important", "off") or not _window_ok(body):
            _empty(handler, 400)
            return True
        STATE["notify"] = body
        _empty(handler)
        return True
    m = re.match(r"^/api/notify/(senders|threads)/(.+)$", path)
    if m:
        body = _body(handler)
        _write("PUT", path, body)
        allowed = {"senders": ("always", "never"), "threads": ("follow", "mute")}[m.group(1)]
        value = body.get("value")
        if value is not None and value not in allowed:
            _empty(handler, 400)
            return True
        _override(m.group(1), unquote(m.group(2)).lower(), value)
        _empty(handler)
        return True
    if path == "/api/settings/vacation":
        body = _body(handler)
        _write("PUT", path, body)
        if body.get("enabled") and not str(body.get("body", "")).strip():
            handler._send({"error": "invalid_body"}, 400)
            return True
        body["version"] = STATE["vacation"]["version"] + 1
        STATE["vacation"] = body
        handler._send(body)
        return True
    return False


def delete(handler):
    path = urlparse(handler.path).path
    for prefix, key in (("/api/auth/sessions/", "sessions"), ("/api/auth/app-passwords/", "app_passwords")):
        if path.startswith(prefix):
            wanted = unquote(path[len(prefix):])
            _write("DELETE", path, None)
            rows = STATE[key]
            if not any(r["id"] == wanted for r in rows):
                _empty(handler, 404)
                return True
            STATE[key] = [r for r in rows if r["id"] != wanted]
            _empty(handler)
            return True
    return False


def post(handler):
    path = urlparse(handler.path).path
    if path == "/api/auth/sessions/end-others":
        _write("POST", path, None)
        others = [s for s in STATE["sessions"] if not s["current"]]
        STATE["sessions"] = [s for s in STATE["sessions"] if s["current"]]
        handler._send({"ended": len(others)})
        return True
    if path == "/api/masked-emails":
        body = _body(handler)
        _write("POST", path, body)
        n = len(STATE["masks"]) + 1
        mask = {"id": f"m{n}", "email": f"new.mask{n}@golia.jp", "owner": "me@golia.jp",
                "state": "enabled", "forDomain": body.get("forDomain", ""),
                "description": body.get("description", ""), "url": body.get("url"),
                "createdAt": BASE_TS + n, "createdBy": "web", "lastMessageAt": None}
        STATE["masks"].insert(0, mask)
        handler._send(mask, 201)
        return True
    if path == "/api/senders/reclassify":
        body = _body(handler)
        _write("POST", path, body)
        if body.get("bucket") not in ("inbox", "notifications", "promotions") or "@" not in body.get("sender", ""):
            _empty(handler, 400)
            return True
        handler._send({"moved": 2, "correction_id": "c-1"})
        return True
    return False
