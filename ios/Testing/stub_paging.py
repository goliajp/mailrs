"""The paged list fixtures, answered the way `/api/conversations` pages.

Two ways to ask for the next page, as the server takes them
(`crates/core-api/src/paging.rs`):

- `before_ts`: the old keyset, `last_date < before_ts`, newest first.
  Kept for clients that have not moved to the cursor.
- `cursor`: the last row's `cursor` from the page before. Every row
  carries one, `"<last_date>:<ord>"`; within one second rows go in
  `ord` order, and the next page starts strictly after the cursor's
  row, so a second that spans two pages loses nothing and repeats
  nothing.

Both at once is a 400, as the server answers it.

`Paged` is 120 threads with rows 48-52 sharing one second. `SameSecond`
is five threads that all share one second, served two to a page: a
client paging on the boundary second (`before_ts = oldest + 1`) gets the
same two rows back, takes that for the end, and never shows the other
three.
"""

from urllib.parse import parse_qs, urlparse

BASE_TS = 1754400000

FOLDERS = ("Paged", "SameSecond")

# the most the SameSecond fixture serves at once, whatever the client asks
SAME_SECOND_PAGE = 2


def _row(i, prefix, subject, ts):
    return {
        "thread_id": f"{prefix}{i}", "subject": f"{subject} {i}",
        "participants": [f"sender{i}@example.com"], "message_count": 1,
        "unread_count": 0, "last_date": ts, "category": "inbox",
        "flagged": False, "snippet": f"body {i}", "pinned": False,
        "archived": False, "importance_level": "normal",
        "importance_score": 0.0, "requires_action": False,
        "received_count": 1, "sent_count": 0,
    }


def _paged():
    rows = []
    for i in range(120):
        ts = BASE_TS - i * 3600
        if 48 <= i <= 52:
            ts = BASE_TS - 48 * 3600
        rows.append(_row(i, "p", "Paged thread", ts))
    return rows


def _same_second():
    return [_row(i, "s", "Same second", BASE_TS) for i in range(5)]


def _ordered(rows):
    """Newest first; within one second, in `ord` order, each row stamped."""
    for i, r in enumerate(rows):
        r["ord"] = i
    rows.sort(key=lambda r: (-r["last_date"], r["ord"]))
    for r in rows:
        r["cursor"] = f"{r['last_date']}:{r['ord']}"
    return rows


def _after(rows, cursor):
    ts, _, ord_ = cursor.partition(":")
    ts, ord_ = int(ts), int(ord_)
    return [r for r in rows
            if r["last_date"] < ts or (r["last_date"] == ts and r["ord"] > ord_)]


def answer(handler, folder):
    """Answer `GET /api/conversations?folder=<Paged|SameSecond>`."""
    query = parse_qs(urlparse(handler.path).query)
    limit = int(query.get("limit", ["50"])[0])
    before = query.get("before_ts", [None])[0]
    cursor = query.get("cursor", [None])[0]
    if before is not None and cursor is not None:
        handler._send({"error": "give before_ts or cursor, not both"}, status=400)
        return
    if folder == "SameSecond":
        rows = _ordered(_same_second())
        limit = min(limit, SAME_SECOND_PAGE)
    else:
        rows = _ordered(_paged())
    if cursor is not None:
        rows = _after(rows, cursor)
    if before is not None:
        rows = [r for r in rows if r["last_date"] < int(before)]
    page = []
    for r in rows[:limit]:
        out = dict(r)
        del out["ord"]
        page.append(out)
    handler._send(page)
