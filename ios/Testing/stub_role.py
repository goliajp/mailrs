"""Who the signed-in account is, as `GET /api/auth/me` answers it.

`POST /debug/role/<plain|admin|super>` changes the role; `/debug/reset` puts
it back to `super`, which every older test assumes. The admin routes refuse
the way the server does for that role: a plain account gets 403 on all of
`/api/admin/`, an administrator on the routes kept to a super administrator.
`GET /debug/role-state` says how many times the app asked who it is, which
is what shows that it asked again.
"""

import re

ROLE = ["super"]
ME_READS = [0]

# crates/webapi/src/handlers/kevy_util.rs — SUPER_ONLY
SUPER_ONLY = (
    "/api/admin/queues",
    "/api/admin/suppressions",
    "/api/admin/audit-log",
    "/api/admin/audit/",
    "/api/admin/auth-events",
    "/api/admin/spam-feedback-stats",
)


def handle(handler):
    """Answer `/api/auth/me` and the debug routes; True when this was one."""
    path = handler.path.split("?")[0]
    m = re.match(r"^/debug/role/(plain|admin|super)$", path)
    if m:
        ROLE[0] = m.group(1)
        handler._send({"ok": True})
        return True
    if path == "/debug/role-state":
        handler._send({"role": ROLE[0], "me_reads": ME_READS[0]})
        return True
    if path == "/api/auth/me":
        ME_READS[0] += 1
        role = ROLE[0]
        handler._send({
            "address": "me@golia.jp",
            "display_name": "Test",
            "permissions": {"plain": [], "admin": ["admin.accounts"], "super": ["*"]}[role],
            "accessible_domains": ["golia.jp"],
            "send_as": [],
            "is_admin": role != "plain",
            "is_super": role == "super",
        })
        return True
    return False


def refused(handler):
    """Answer 403 and return True when the role may not call this route."""
    path = handler.path.split("?")[0]
    if not path.startswith("/api/admin/"):
        return False
    role = ROLE[0]
    sieve = path.startswith("/api/admin/accounts/") and path.endswith("/sieve")
    if role == "plain" or (role == "admin" and (sieve or path.startswith(SUPER_ONLY))):
        handler._send({"error": "forbidden"}, status=403)
        return True
    return False


def reset():
    ROLE[0] = "super"
    ME_READS[0] = 0
