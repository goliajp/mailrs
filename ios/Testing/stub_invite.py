"""An all-day invitation, on request.

The invitation on message 2 is a timed meeting. `POST /debug/invite-all-day`
turns it into an all-day one, the way `crates/intake/src/invite.rs` files
it: no instants (`dtstart_utc` and `dtend_utc` are null, because a date
has no offset and giving it one moves it a day), the dates instead in
`dtstart_date` and `dtend_date` (the day after the last, as RFC 5545
writes DTEND), and `dtstart` as `{"Date": …}`. `?on=0` and `/debug/reset`
put the timed meeting back.
"""

import copy
from urllib.parse import parse_qs, urlparse

ARMED = [False]

FIRST_DAY = "2026-11-03"
DAY_AFTER_LAST = "2026-11-05"


def arm(handler):
    """Handle `POST /debug/invite-all-day[?on=0]`; True when this was it."""
    if handler.path.split("?")[0] != "/debug/invite-all-day":
        return False
    ARMED[0] = parse_qs(urlparse(handler.path).query).get("on", ["1"])[0] != "0"
    handler._send({"ok": True})
    return True


def payload(timed):
    """The invitation message 2 carries: `timed`, or its all-day form."""
    if not ARMED[0]:
        return timed
    out = copy.deepcopy(timed)
    out["summary"] = "Offsite"
    out["dtstart"] = {"Date": FIRST_DAY}
    out["dtend"] = {"Date": DAY_AFTER_LAST}
    out["dtstart_utc"] = None
    out["dtend_utc"] = None
    out["dtstart_date"] = FIRST_DAY
    out["dtend_date"] = DAY_AFTER_LAST
    out["join_url"] = None
    return out


def reset():
    ARMED[0] = False
