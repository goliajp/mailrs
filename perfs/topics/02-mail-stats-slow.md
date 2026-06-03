# Topic 02: `/api/mail/stats` 174 ms TTFB for a 0.5 KB JSON

**Status:** fixed (v1.4.26)
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

### 2026-04-20 — EXPLAIN of each subquery the handler runs

`data/2026-04-20/explain-b5.txt`:

| query | exec time |
|---|---:|
| `count_messages` (`SELECT COUNT(*) FROM messages JOIN mailboxes`) | 6 ms |
| `count_unseen` (groups + per-row NOT EXISTS + per-group SubPlan) | **107 ms** |
| `user_storage_usage` (`SELECT SUM(size)`) | 13 ms |
| `list_conversation_categories` (group by ea.category) | 57 ms |

Sum ≈ 183 ms, matches the observed ~175 ms TTFB.

`count_unseen` duplicates the same SubPlan pattern that
`list_conversations` had (per-row spam/scam exclusion, per-group
last-sender SubPlan in HAVING). I tried the same fix-a / fix-c shape
on it and it **made things slower**, three runs each:

```
baseline (SubPlan + NOT EXISTS):              107 / 109 / 107 ms
fix-a only (array_agg in HAVING):             124 / 124 / 126 ms
fix-c (LEFT JOIN ea + array_agg):             126 ms (single run)
```

The reason `count_unseen` doesn't benefit: this query's
GroupAggregate output is small enough that the per-thread index probes
in SubPlan 7 amortise well, and the array_agg sort cost across rows
within each group exceeds the SubPlan cost. The planner already chose
the best shape for this query.

Conclusion: **stop running it on every dashboard tick** rather than
trying to make it faster.

## Decision

Cache the entire `MailStats` JSON in kevy for 30 s. The dashboard
refreshes mail/stats every 60 s, so a 30 s TTL absorbs the loop and
any tab-focus refetches without user-perceptible staleness.

`crates/server/src/web/conversations.rs::get_mail_stats`:

- only the simple single-user case is cached; the rare cross-domain
  view (`?domains=…`) skips cache to avoid per-key invalidation work
- cache key `mail:stats:v1:{user}`, TTL 30 s
- silently falls back to recompute on cache miss / kevy error
- `MailStats` and `CategoryCount` gain `Deserialize`

Released as v1.4.26 on 2026-04-20.

## Verification

Post-deploy curl on prod:

```
first hit (cache miss):    187 ms TTFB / 214 ms total
subsequent (cache hits):   12-15 ms TTFB / 38-42 ms total
```

So:

| state | TTFB before (v1.4.25) | TTFB after (v1.4.26) | Δ |
|---|---:|---:|---:|
| cache hit (steady-state) | 175 ms | **12 ms** | **−163 ms (−93%)** |
| cache miss (every 30 s) | 175 ms | 187 ms | +12 ms (one-shot, hidden by background refresh) |

Dashboard's `Promise.all([conversations, stats, folders])` is no longer
gated by stats — `?limit=200` (~258 ms) is now the slowest of the
three on warm cache. The user sees the dashboard fully rendered ~163 ms
sooner from the second visit onward.
