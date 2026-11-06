"""An all-day or a recurring invitation, on request.

The invitation on message 2 is a timed meeting. `POST /debug/invite-all-day`
turns it into an all-day one, the way `crates/intake/src/invite.rs` files
it: no instants (`dtstart_utc` and `dtend_utc` are null, because a date
has no offset and giving it one moves it a day), the dates instead in
`dtstart_date` and `dtend_date` (the day after the last, as RFC 5545
writes DTEND), and `dtstart` as `{"Date": …}`.

`POST /debug/invite-recurring` turns it into a weekly series instead: every
Thursday at 15:00 Pacific, fifty minutes, with the Windows `TZID` Exchange
writes. The server answers a series with what
`calendar_events::series::annotate` adds when the message is read: the
rule in words (`recurrence`) and the first occurrence that has not ended
(`next_occurrence`, its instants and the organiser's wall clock). The
series is placed around the moment of the request — it began three weeks
ago and ends in half a year — so the next occurrence is always ahead and
the test does not start failing on a fixed date.

`?on=0` and `/debug/reset` put the plain timed meeting back.
"""

import copy
from datetime import datetime, timedelta, timezone
from urllib.parse import parse_qs, urlparse
from zoneinfo import ZoneInfo

ALL_DAY = [False]
RECURRING = [False]

FIRST_DAY = "2026-11-03"
DAY_AFTER_LAST = "2026-11-05"

PACIFIC = ZoneInfo("America/Los_Angeles")
LENGTH = timedelta(minutes=50)


def arm(handler):
    """Handle `POST /debug/invite-all-day` and `/debug/invite-recurring`
    (each with `?on=0`); True when this was one of them."""
    path = handler.path.split("?")[0]
    on = parse_qs(urlparse(handler.path).query).get("on", ["1"])[0] != "0"
    if path == "/debug/invite-all-day":
        ALL_DAY[0] = on
    elif path == "/debug/invite-recurring":
        RECURRING[0] = on
    else:
        return False
    handler._send({"ok": True})
    return True


def payload(timed):
    """The invitation message 2 carries: `timed`, or the form armed."""
    if ALL_DAY[0]:
        return _all_day(timed)
    if RECURRING[0]:
        return _recurring(timed, datetime.now(timezone.utc))
    return timed


def _all_day(timed):
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


def _thursday_three_weeks_before(now):
    local = now.astimezone(PACIFIC) - timedelta(weeks=3)
    day = local.date() - timedelta(days=(local.weekday() - 3) % 7)
    return datetime(day.year, day.month, day.day, 15, 0, tzinfo=PACIFIC)


def _utc(t):
    return t.astimezone(timezone.utc).isoformat()


def _recurring(timed, now):
    first = _thursday_three_weeks_before(now)
    last_day = (first + timedelta(weeks=26)).date()
    # each occurrence is 15:00 on the organiser's wall clock, so its UTC
    # instant moves when Pacific time changes
    start = first
    while start + LENGTH <= now:
        start = datetime.combine((start + timedelta(weeks=1)).date(), first.timetz())
    out = copy.deepcopy(timed)
    out["summary"] = "Weekly product sync"
    out["dtstart"] = {"Zoned": {"local": first.strftime("%Y-%m-%dT%H:%M:%S"),
                                "tz_name": "Pacific Standard Time"}}
    out["dtend"] = {"Zoned": {"local": (first + LENGTH).strftime("%Y-%m-%dT%H:%M:%S"),
                              "tz_name": "Pacific Standard Time"}}
    out["dtstart_utc"] = _utc(first)
    out["dtend_utc"] = _utc(first + LENGTH)
    out["rrule"] = f"FREQ=WEEKLY;BYDAY=TH;UNTIL={last_day.strftime('%Y%m%d')}T235959Z"
    out["recurrence"] = {"freq": "WEEKLY", "interval": 1, "by_day": ["TH"],
                         "by_month_day": [], "by_month": [], "count": None,
                         "until": last_day.isoformat(), "plain": True}
    out["next_occurrence"] = {
        "start": _utc(start),
        "end": _utc(start + LENGTH),
        "organiser_local": start.strftime("%Y-%m-%dT%H:%M:%S"),
        "organiser_offset_secs": int(start.utcoffset().total_seconds()),
    }
    return out


def reset():
    ALL_DAY[0] = False
    RECURRING[0] = False
