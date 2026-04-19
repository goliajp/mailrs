# Topic 01: `/api/conversations` 340–400 ms TTFB

**Status:** mostly fixed (v1.4.21 fix-a, v1.4.25 fix-c) — fix-d snapshot table still open
**Severity:** high
**First observed:** 2026-04-19 (TREE.md, /dashboard + /mail)
**Owner:** —

## Symptom

The unfiltered list endpoint dominates two of the most-loaded pages.

| call site | request | size | TTFB | total |
|---|---|---:|---:|---:|
| /dashboard initial | `GET /api/conversations?limit=200` | 73.3 KB | 354 ms | 404 ms |
| /mail initial | `GET /api/conversations?limit=50` | 36.1 KB | 340 ms | 379 ms |
| /mail tab → Unread | `GET /api/conversations?limit=50&unread=true` | 0 B | 207 ms | 236 ms |
| /mail tab → Starred | `GET /api/conversations?limit=50&starred=true` | 0 B | 203 ms | 233 ms |
| /mail tab → Sent | `GET /api/conversations?limit=50&folder=Sent` | 28.6 KB | 21 ms | 58 ms |

Server work for `?limit=50` is ~140 ms heavier than the same endpoint with `?unread=true`, and ~10 ms heavier than `?limit=200` from the same dataset — i.e. the cost is **not** dominated by row count. It's per-thread enrichment in the unfiltered "All" path.

## Reproduction

```bash
cd perfs
TOKEN=… ./scripts/timing.sh "all-50"     GET 'https://mail.golia.ai/api/conversations?limit=50'
TOKEN=… ./scripts/timing.sh "all-200"    GET 'https://mail.golia.ai/api/conversations?limit=200'
TOKEN=… ./scripts/timing.sh "unread-50"  GET 'https://mail.golia.ai/api/conversations?limit=50&unread=true'
TOKEN=… ./scripts/timing.sh "sent-50"    GET 'https://mail.golia.ai/api/conversations?limit=50&folder=Sent'
```

## Hypotheses

1. **N+1 enrichment in the unfiltered branch.** `?folder=Sent` is fast (58 ms) and `?unread=true` is faster than the default; the default path likely does extra per-thread work (importance, snippet, last-message hydrate, dedup across folders). Confirm by reading `crates/server/src/web/conversations.rs::list_conversations` and tracing what runs only in the `folder.is_none() && !unread && !starred` branch.
2. **Missing index on the thread-ranking query.** `EXPLAIN ANALYZE` against prod replica should show whether the planner is doing a seq scan or sort-without-index.
3. **Cross-folder dedup query.** "All" hides own sends from Sent → may be doing a NOT EXISTS / anti-join over a non-indexed column.
4. **Per-row LLM-derived columns being lazy-computed at read time.** If `importance_level` / `summary` is computed inline when missing, hot threads might be cheap, cold ones expensive. Check whether TTFB scales with row count of "stale" threads.

## Investigation log

### 2026-04-19 — code read of `mailrs-mailbox::store::list_conversations`

Source: `crates/mailbox/src/store.rs:777-971` (the dynamically-built SQL).

The query shape is:

```
SELECT
  m.thread_id,
  MAX(m.subject),
  string_agg(DISTINCT m.sender, ','),
  COUNT(DISTINCT … message_id …),                                 -- count_expr
  COUNT(DISTINCT … unread …),                                     -- unread_expr
  MAX(m.internal_date),
  COALESCE((SELECT ea.category FROM email_analysis ea
              JOIN messages m2 ON ea.message_id = m2.id
              WHERE m2.thread_id = m.thread_id
              ORDER BY m2.internal_date DESC LIMIT 1), 'general'),  -- (1)
  BOOL_OR((m.flags & 4) != 0),
  COALESCE(
    (SELECT ea_snip.summary FROM email_analysis …
       WHERE m_snip.thread_id = m.thread_id … ORDER BY m_snip.internal_date DESC LIMIT 1),
    (SELECT LEFT(m3.text_body, 120) FROM messages m3
       WHERE m3.thread_id = m.thread_id … ORDER BY m3.internal_date DESC LIMIT 1),
    ''),                                                           -- (2)+(3)
  BOOL_OR(m.pinned),
  BOOL_OR(m.archived),
  COALESCE((SELECT m_imp.importance_level FROM messages m_imp
              WHERE m_imp.thread_id = m.thread_id
              ORDER BY m_imp.importance_score DESC NULLS LAST LIMIT 1), 'normal'), -- (4)
  COALESCE(MAX(m.importance_score), 0.0),
  COALESCE(BOOL_OR((SELECT ea_act.requires_action FROM email_analysis ea_act
                      WHERE ea_act.message_id = m.id)), false),     -- (5) per-row, NOT per-thread
  COALESCE((SELECT m_last.sender FROM messages m_last
              WHERE m_last.thread_id = m.thread_id
              ORDER BY m_last.internal_date DESC LIMIT 1), '')      -- (6)
FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
WHERE …
  AND NOT EXISTS (SELECT 1 FROM snoozed_conversations sc …)         -- (7) per-row
  AND NOT EXISTS (SELECT 1 FROM email_analysis ea_ex
                    WHERE ea_ex.message_id = m.id
                      AND ea_ex.category IN ('spam','scam'))         -- (8) per-row, only when no category filter
GROUP BY m.thread_id
HAVING …
   AND LOWER(COALESCE((SELECT m_last.sender …
                          ORDER BY m_last.internal_date DESC LIMIT 1), ''))
       NOT LIKE '%' || LOWER($N) || '%'                              -- (9), only when folder != 'Sent'
ORDER BY BOOL_OR(m.pinned) DESC, MAX(m.internal_date) DESC
LIMIT $L
```

So per query: **6 thread-correlated subqueries (1–4, 6, 9)** evaluated once per output row, plus **3 row-correlated subqueries (5, 7, 8)** evaluated once per scanned message row, plus the `string_agg(DISTINCT m.sender, ',')` which forces a sort within each group.

Why the measurements line up:

- `?folder=Sent` is the fast variant (58 ms) because subquery (9) — the most expensive HAVING clause — is **disabled** when the caller asks for Sent (`folder == Some("Sent")`); plus the Sent mailbox is much smaller, so the per-message correlated subqueries (5, 7, 8) run far fewer times.
- `?unread=true` (236 ms) is faster than the default because the `unread_count > 0` HAVING eliminates most threads after grouping, but subqueries 5/7/8 still scan the whole user message set in the WHERE, so it doesn't get all the way down to Sent's number.
- `?limit=200` is barely slower than `?limit=50` (404 vs 379) because the per-message subqueries dominate over the per-thread ones for this account; thread count above the LIMIT is not the bottleneck.

Indexes that exist on `messages` (from `scripts/init-schema.sql` + `migrate-007-attachment-content.sql`):

- `idx_messages_thread (thread_id)`
- `idx_messages_thread_date (thread_id, internal_date DESC)` ← the hot index for subqueries 1–4, 6, 9
- `idx_messages_date (mailbox_id, date_epoch DESC)`
- `idx_messages_importance (mailbox_id, importance_level, internal_date DESC)`

The thread-correlated subqueries each get a cheap index probe, but **5–6 probes × 50 threads ≈ 300 cheap probes per request**, which on a single Postgres connection adds up to the observed 340 ms TTFB even with everything in cache.

### Hypotheses, refined

| # | claim | status |
|---|---|---|
| 1 | Per-thread correlated subqueries dominate; index probes are cheap individually but stack up. | **likely root cause** — supported by code, by `folder=Sent` skipping (9), and by `unread/starred` being faster purely from row count reduction. |
| 2 | Missing index. | **ruled out** — `idx_messages_thread_date` covers every hot subquery. |
| 3 | Per-row `NOT EXISTS` (snoozed, spam/scam) walks the whole user mailbox even before grouping. | **contributing** — explains why `?limit=N` doesn't help; needs EXPLAIN to quantify. |
| 4 | Lazy LLM-derived columns. | **ruled out** — the SQL only reads pre-computed columns; nothing computes on the fly. |

### Fix candidates

| approach | upside | downside |
|---|---|---|
| **A. LATERAL join for "latest message per thread"** — replace subqueries 1, 2, 4, 6, 9 with one `LATERAL (SELECT … FROM messages WHERE thread_id = m.thread_id ORDER BY internal_date DESC LIMIT 1)` and another for `email_analysis`. | Single SQL change; planner gets to see the join shape; reuses `idx_messages_thread_date`. | Still O(threads) lookups, just one set of them — should drop probes from ~250 to ~50. |
| **B. CTE that pre-computes `(thread_id, latest_message_id, latest_internal_date)`** then joins for all the per-thread fields. | Cleanest single-pass plan. | Slightly more code; need to verify planner doesn't materialise the CTE for the wrong shape. |
| **C. Derive a `thread_summary` snapshot table** updated on message insert/update, holding all per-thread aggregates. The list endpoint becomes a flat `SELECT` from `thread_summary` joined to `mailboxes`. | Sub-50 ms target reachable; aligns with `data-architecture.md` (facts vs derivations — the per-thread aggregate is a derivation). | Maintenance work: trigger or app-level write path on every message insert/flag change. Risk of drift, needs a backfill script. |
| **D. Move the spam/scam exclusion out of `NOT EXISTS` into a join with a partial index `WHERE category IN ('spam','scam')`.** | Kills the per-row subquery (8). | Smaller wins; doesn't address subqueries 1–6/9. |

Recommendation: **A first** (low-risk, single-file change, immediate win), measure, then evaluate whether C is worth the operational cost.

### 2026-04-19 — EXPLAIN ANALYZE on prod (data file: `data/2026-04-19/explain-conversations-default.txt`)

`EXECUTE q ('lihao@golia.jp', 50, 'lihao@golia.jp')` — 18762 messages, 18262 threads on this account.

```
Limit  …  (actual time=350.439..352.279 rows=50)
  Buffers: shared hit=153592, temp read=922 written=923   ← 7.4 MB external sort to disk
  └─ Sort  …  Sort Method: top-N heapsort  Memory: 50kB
       └─ GroupAggregate  (actual time=74.146..341.203 rows=16609)
              Group Key: m.thread_id
              Filter: ((NOT bool_or(m.archived))
                   AND (lower(COALESCE((SubPlan 7), '')) !~~ '%lihao@golia.jp%'))
              Rows Removed by Filter: 186
              └─ Incremental Sort … rows=17280
                   └─ Nested Loop Anti Join … rows=17280
                        └─ Merge Anti Join … rows=18762
                              ├─ Sort  rows=18762  Sort Method: external merge  Disk: 7376kB
                              │    └─ Hash Join  Seq Scan on messages m  rows=19389
                              │       (Filter: thread_id <> '')
                              └─ snoozed_conversations  rows=0

         SubPlan 7 (HAVING — hide my latest sender)
            Index Scan idx_messages_thread_date    loops=16793   ← runs per group
         SubPlan 5 (SELECT — requires_action)
            Index Scan email_analysis_pkey         loops=17280   ← runs per row
         SubPlan 8 (WHERE — NOT EXISTS spam/scam)
            Index Scan email_analysis_pkey         loops=18762   ← runs per row
         SubPlan 1, 2, 3, 4, 6 (SELECT — category/snippet/importance/last_sender)
            Index Scan idx_messages_thread_date    loops=50      ← only on LIMIT survivors

Planning Time: 4.7 ms
Execution Time: 354.4 ms                                           ← matches observed TTFB exactly
```

Three things stack up to the 354 ms:

1. **`Seq Scan on messages` returns 19389 rows**, then sorts them by `thread_id` for the merge join. The sort spills to disk (`external merge Disk: 7376 kB`). This alone is a chunk of the time and is purely a `work_mem` problem.
2. **SubPlan 7 (HAVING `hide_my_latest`) runs once per group — 16793 loops**, hitting `idx_messages_thread_date` 16 793 times. This is the single biggest piece of CPU. It runs *before* `LIMIT 50`, so paging never helps. **Disabling it for `folder=Sent` is exactly why `?folder=Sent` is 6× faster (58 ms)**.
3. **SubPlans 5 and 8 fire once per scanned message row** (17 280 / 18 762 loops). Each is a fast index probe (~1 µs) but the sheer count adds another ~50 ms.

The five "thread-correlated" subqueries in the SELECT list (1, 2, 3, 4, 6) are *not* the problem — Postgres only evaluates them on the 50 surviving rows after `GROUP BY HAVING ORDER BY LIMIT`. They cost ~5 ms total.

### Fix candidates (revised after EXPLAIN)

Listed in order of effort vs. payoff:

| | change | expected win | risk |
|---|---|---|---|
| **a** | Move `last_sender` from a HAVING SubPlan to an aggregate expression: `(array_agg(m.sender ORDER BY m.internal_date DESC))[1]`. Same data, computed once during the GroupAggregate pass instead of 16 793 separate index scans. | Eliminates SubPlan 7 (~80–100 ms). | Low: same semantics, single SQL change. |
| **b** | Bump Postgres `work_mem` from default (4 MB) to ≥ 16 MB so the 7.4 MB sort stays in memory. | Eliminates disk I/O on this query (~30–50 ms) and on every other multi-row sort. | Low (config), but is a global setting — scale with concurrency. |
| **c** | Hoist `requires_action` and the spam/scam exclusion into a single `LEFT JOIN email_analysis_latest` derived from `LATERAL (SELECT … FROM messages WHERE thread_id = … ORDER BY internal_date DESC LIMIT 1)`. | Eliminates SubPlan 5 + 8 (~50 ms). | Medium: bigger SQL rewrite. |
| **d** | Materialise a `thread_summary` snapshot table (one row per `(user_address, thread_id)`) updated on every message insert/flag change. Endpoint becomes a flat indexed select. | Sub-50 ms target reachable, hardens future scaling. | High: write-path changes, backfill, drift management. Aligns with `data-architecture.md` (derivation snapshot). |

Recommendation: **a + b first** (one PR each, both reversible). Re-measure. Only consider c/d if `?limit=50` doesn't drop under ~100 ms.

## Decision

Two ships against `crates/mailbox/src/store.rs::list_conversations`:

**v1.4.21 — fix-a** (per-group SubPlan 7 → ordered aggregate):
- SELECT-list `last_sender` and HAVING-clause `hide_my_latest` both replaced
  with `COALESCE((array_agg(m.sender ORDER BY m.internal_date DESC))[1], '')`.
- Computed once during `GroupAggregate` instead of 16 793 separate index
  probes per request.

**v1.4.25 — fix-c** (per-row SubPlan 5 + 8 → LEFT JOIN):
- `LEFT JOIN email_analysis ea ON ea.message_id = m.id` added once.
- `BOOL_OR((SELECT requires_action FROM email_analysis ea_act ...))` →
  `BOOL_OR(ea.requires_action)` (kills SubPlan 5, 17 280 loops).
- `NOT EXISTS (SELECT 1 FROM email_analysis ea_ex WHERE category IN
  ('spam','scam'))` → `COALESCE(ea.category, 'general') NOT IN
  ('spam','scam')` (kills SubPlan 8, 18 762 loops).
- Category filter `m.id IN (SELECT … WHERE category = $N)` simplifies to
  `ea.category = $N`.

fix-b (`work_mem` to 16+ MB) deferred — server config tuning, separate
review. fix-d (snapshot `thread_summary` table) still open as a
medium-term lever.

fix-b (work_mem) deferred — needs broader review of Postgres tuning, only
buys ~12 ms on top of fix-a in the EXPLAIN.

fix-c (LATERAL email_analysis to kill SubPlan 5 + 8) and fix-d (snapshot
table) left open. Will only be tackled if the post-deploy TTFB on /mail
remains in the user-perceptible range (>200 ms).

## Verification

### v1.4.21 — fix-a (per-group SubPlan 7 → ordered aggregate)

Three runs each, median TTFB shown:

| request | before (v1.4.20) | after (v1.4.21) | Δ |
|---|---:|---:|---:|
| `/api/conversations?limit=50` | 340 ms TTFB / 379 ms total | **278 ms / 324 ms** | −62 / −55 ms (−18%) |
| `/api/conversations?limit=200` | 354 ms TTFB / 404 ms total | **273 ms / 326 ms** | **−81 / −78 ms (−23%)** |
| `/api/conversations?limit=50&unread=true` | 207 ms / 236 ms | 229 ms / 254 ms | noise |
| `/api/conversations?limit=50&folder=Sent` (control) | 21 ms / 58 ms | 21 ms / 60 ms | unchanged ✓ |

### v1.4.25 — fix-c (per-row SubPlan 5+8 → LEFT JOIN)

EXPLAIN on prod (data: `data/2026-04-20/explain-b4.txt`):
- **before fix-c**: 288 ms execution
- **after fix-c**:  249 ms execution (−14%)
- after fix-c + work_mem 32 MB: 243 ms (extra −2%, eliminates 7.5 MB
  external-merge sort to disk; deferred to a separate config change)

Post-deploy curl, comparing to the immediately-prior baseline (v1.4.24,
which already had fix-a applied):

| request | v1.4.24 TTFB | v1.4.25 TTFB | Δ |
|---|---:|---:|---:|
| `?limit=50` | 270 ms | 267 ms | within noise |
| `?limit=200` | 271 ms | **258 ms** | −13 ms |
| `?limit=50&unread=true` | 231 ms | **217 ms** | −14 ms |
| `?limit=50&starred=true` | 227 ms | **218 ms** | −9 ms |
| `?limit=50&section=important` | 266 ms | **222 ms** | **−44 ms** |
| `?limit=50&category=spam` | 75 ms | 70 ms | −5 ms |
| `?limit=50&folder=Sent` (control) | 21 ms | 21 ms | unchanged ✓ |

fix-c's effect is bigger on filtered variants because they have fewer
final groups but the same large pool of message rows feeding SubPlan 5+8;
removing those subqueries removes a per-row cost that the smaller HAVING
result can no longer amortise.

### Total improvement on /api/conversations (v1.4.20 → v1.4.25)

| variant | initial | after fix-a + B3 + fix-c | total Δ |
|---|---:|---:|---:|
| `?limit=200` (dashboard) | 354 ms TTFB / 404 ms total | 258 / 312 ms | **−96 / −92 ms (−27% / −23%)** |
| `?limit=50` (/mail initial) | 340 / 379 ms | 267 / 306 ms | −73 / −73 ms (−21% / −19%) |
| `?section=important` | 376 / 581 ms | 222 / 261 ms | **−154 / −320 ms (−41% / −55%)** |
| `?unread=true` | 207 / 236 ms | 217 / 244 ms | within noise (already small) |
| `?folder=Sent` (control) | 21 / 58 ms | 21 / 60 ms | unchanged ✓ |

### Remaining gap

`/api/conversations?limit=50` is still ~270 ms TTFB. The remaining cost
is mostly the GroupAggregate over 17k message rows + the external-merge
sort to disk (still happens at default `work_mem`).

Next levers:

1. **fix-b (raise `work_mem` from 4 MB to 16+ MB)** — eliminates the
   7.5 MB sort to disk, ~12 ms saved here and free wins on every other
   multi-row aggregation. Server-wide setting, should be reviewed.
2. **fix-d (`thread_summary` snapshot table)** — strategic refactor.
   Brings the endpoint to flat-select latency (sub-50 ms) and stops
   being O(threads in user's mailbox). Aligns with the `data-architecture.md`
   "facts vs derivations" principle (per-thread aggregate is a derivation).
