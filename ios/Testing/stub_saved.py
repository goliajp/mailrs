"""Saved searches, as `handlers/saved_searches.rs` lists them.

`GET /api/saved-searches` answers a bare array of `{id, name, query,
created_at}`. The phones only run them; making and deleting stays on the
web. One query matches only some of the conversation fixtures, so
running it visibly narrows the list; the other uses operators and has a
name longer than a phone row, so a client that drops the query or wraps
the name badly shows it.
"""

SAVED = [
    {"id": "ss_ref", "name": "Invoices", "query": "請求", "created_at": 1754400000},
    {"id": "ss_ops", "name": "Attachments from the quarterly review committee, every year",
     "query": "from:chair@example.com has:attachment", "created_at": 1754300000},
]


def get(handler):
    """Answer the list. False for any other request."""
    if handler.path.split("?")[0] != "/api/saved-searches":
        return False
    handler._send(SAVED)
    return True
