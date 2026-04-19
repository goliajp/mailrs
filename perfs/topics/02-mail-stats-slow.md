# Topic 02: `/api/mail/stats` 174 ms TTFB for a 0.5 KB JSON

**Status:** open
**Severity:** medium
**First observed:** 2026-04-19 (TREE.md, /dashboard)
**Owner:** —

## Symptom

| call site | request | size | TTFB | total |
|---|---|---:|---:|---:|
| /dashboard | `GET /api/mail/stats` | 0.5 KB | 174 ms | 202 ms |

Payload is tiny (categories + storage_bytes + total_messages + unread_messages). A 174 ms TTFB on a 0.5 KB response is a server-side computation cost, not bandwidth.

## Reproduction

```bash
TOKEN=… ./scripts/timing.sh "stats" GET 'https://mail.golia.ai/api/mail/stats'
```

## Hypotheses

1. **Unbounded `COUNT(*)` over the messages table** for `total_messages` / `unread_messages`. Common pattern that tanks once row count grows. Check whether either uses a partial / covering index.
2. **`storage_bytes` walks the maildir** instead of reading a cached size. Filesystem walks are O(messages).
3. **`categories` query joins messages with no precomputed aggregate** and groups every call.

Each of these is verifiable with `EXPLAIN` against the prod DB and/or strace of the handler.

## Investigation log

- 2026-04-19 — flagged from cold-network curl. Not yet inspected.

## Decision

—

## Verification

—
