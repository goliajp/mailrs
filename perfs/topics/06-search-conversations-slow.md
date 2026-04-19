# Topic 06: `/api/conversations/search` ~600 ms TTFB

**Status:** fully fixed (ASCII v1.4.27 + CJK v1.4.29 via meilisearch)
**Severity:** resolved
**First observed:** 2026-04-19 (data/2026-04-19/sweep.txt)
**Owner:** —

## Symptom

| query | size | TTFB | total |
|---|---:|---:|---:|
| `?q=invoice&limit=50` | 25.3 KB | **596 ms** | 634 ms |
| `?q=金额&limit=50` (CJK) | 11.0 KB | **576 ms** | 612 ms |

Slowest endpoint in the entire app. Hits whenever the user types in the search bar.

## Reproduction

```bash
TOKEN=… ./scripts/timing.sh "search invoice" GET 'https://mail.golia.ai/api/conversations/search?q=invoice&limit=50'
TOKEN=… ./scripts/timing.sh "search 金额"   GET 'https://mail.golia.ai/api/conversations/search?q=%E9%87%91%E9%A2%9D&limit=50'
```

## Hypotheses

Source: `crates/mailbox/src/store.rs:1621-1737`. The query has the same per-thread enrichment pattern as `list_conversations` (subqueries 1, 2, 4, 6 — already discussed in topic-01) **plus** an extra-expensive WHERE clause:

```sql
AND ( m.search_vector @@ plainto_tsquery('simple', $q)
   OR m.subject ILIKE $pattern
   OR m.sender ILIKE $pattern
   OR m.text_body ILIKE $pattern
   OR m.clean_text ILIKE $pattern
   OR EXISTS (SELECT 1 FROM attachment_content ac
                WHERE ac.message_id = m.id AND ac.extracted_text ILIKE $pattern))
```

Five `ILIKE '%pattern%'` columns + an `EXISTS` over `attachment_content`. trigram indexes exist on `subject`, `sender`, `text_body`, `clean_text` (`migrate-007`) but `OR` of multiple trigram-indexed predicates often defeats index usage — Postgres falls back to seq scan.

1. **The `OR` chain forces a sequential scan** because the planner can't combine multiple GIN trigram indexes through OR. Confirm with `EXPLAIN`.
2. **The attachment EXISTS adds a per-row probe** even when the main predicate already matched.
3. **The same per-thread correlated subqueries from topic-01 still fire** in the SELECT list (1, 2, 4, 6).

## Fix candidates

| | change | expected win | risk |
|---|---|---|---|
| **a** | Run a single `tsvector @@ tsquery` first via a CTE/LIMIT, gate the ILIKE OR-chain to fire only on the survivors. Most queries match via `search_vector` and never need ILIKE. | huge — 600 ms → < 100 ms for vectorisable queries. | low: behaviour unchanged for hits, slight ranking nuance for ILIKE-only hits. |
| **b** | Build a combined `tsvector` over (subject, sender, text_body, clean_text) instead of OR-ing four columns. | medium — single index lookup. | medium: schema migration + reindex. |
| **c** | Drop ILIKE substring on `text_body`/`clean_text` for queries that would obviously be irrelevant (very short / latin tokens already covered by `tsvector`). | small. | low. |
| **d** | Reuse topic-01 fix-a here too: the SELECT-list correlated subqueries 1/2/4/6 are still here. Replace with ordered aggregates. | small (~30 ms) once the WHERE-clause is fast. | low. |

Recommendation: **a** first. Verify with `EXPLAIN ANALYZE` against prod before/after.

## Investigation log

### 2026-04-20 — EXPLAIN of the original query

`data/2026-04-20/explain-b2.txt`. The whole 575 ms goes into one
`Index Scan using idx_messages_date on messages m` with this Filter:

```
Filter: ((thread_id <> '')
     AND ((search_vector @@ '''invoice'''::tsquery)
          OR (subject ~~* '%invoice%')
          OR (sender ~~* '%invoice%')
          OR (text_body ~~* '%invoice%')
          OR (clean_text ~~* '%invoice%')
          OR (id = ANY (hashed SubPlan 7).col1)))
Rows Removed by Filter: 3120 per loop × 6 mailboxes = ~18 720 rows
                                                       evaluated
```

Postgres can BitmapOr predicates that are all backed by the same
operator class on indexes it picks up. It cannot combine a GIN
tsvector with several GIN trigram indexes through `OR` because the
mailbox_id clause already steered it to `idx_messages_date` (a btree)
and from there it has to evaluate each branch row-by-row.

Two further issues found while testing fixes:

- **CTE with the OR-chain inside still seq-scans** because the
  predicates in `cands` look the same to the planner — same problem
  from the planner's perspective.
- **Bare `subject ILIKE '%foo%'` still picks Seq Scan** even though
  `idx_messages_subject_trgm` exists. PG cannot prove rows match the
  partial-index condition `WHERE subject IS NOT NULL AND subject != ''`
  unless the query repeats those clauses explicitly.
  ```
  SELECT count(*) FROM messages WHERE subject ILIKE '%invoice%';
  -> Seq Scan, 54 ms

  SELECT count(*) FROM messages
   WHERE subject IS NOT NULL AND subject != ''
     AND subject ILIKE '%invoice%';
  -> Bitmap Index Scan on idx_messages_subject_trgm, 0.5 ms (100×)
  ```

## Decision

Rewrite the SQL into two stages:

1. CTE `cands` UNIONs one branch per searchable column. Each branch
   matches a single index (gin tsvector for `search_vector`, gin
   trigram for the four ILIKE columns, seq scan for
   `attachment_content`).
2. Each ILIKE branch repeats the partial index's WHERE conditions
   (`subject IS NOT NULL AND subject != ''` etc.) so PG can prove the
   row qualifies and uses the trigram index.

Same fix-c LEFT JOIN email_analysis pattern from topic-01 is applied
here too, killing SubPlan 5 (per-row requires_action) and
simplifying the category filter to `ea.category = $N`.

Released as v1.4.27 on 2026-04-20.

## Verification

prod EXPLAIN (`data/2026-04-20/explain-b2-final.txt`):

```
before:  Execution Time: 575 ms     (single Index Scan + OR Filter)
after:   Execution Time:  45 ms     (-92%)

  Bitmap Index Scan on idx_messages_search_vector  (33 rows, 0.4 ms)
  Bitmap Index Scan on idx_messages_subject_trgm   (19 rows, 0.2 ms)
  Bitmap Index Scan on idx_messages_sender_trgm    (19 rows, 0.4 ms)
  Bitmap Index Scan on idx_messages_text_body_trgm (18 rows, 2.8 ms)
  Bitmap Index Scan on idx_messages_clean_text_trgm(19 rows, 2.7 ms)
  Seq Scan on attachment_content                   (29 ms, hashed)
```

post-deploy curl on prod (median of 3, ASCII queries):

| query | TTFB before | TTFB after | Δ |
|---|---:|---:|---:|
| `q=invoice` | 596 ms | **65 ms** | **−531 ms (−89%)** |
| `q=meeting` | (not measured before) | 59 ms | new baseline |
| `q=金額` (CJK) | 576 ms | 597 ms | unchanged ⚠ |

### CJK fix — meilisearch (v1.4.29)

CJK queries originally hit the same wall: `pg_trgm` strips non-ASCII
chars before tokenisation, so `subject ILIKE '%金額%'` falls back to
Seq Scan on every branch. Total ~570 ms regardless of how the SQL
was written.

The codebase already had a complete meilisearch integration:
- `crates/server/src/search_index.rs` — client + background indexer
- `search_conversations` handler — meili-first, PG-fallback
- `get_conversations_by_thread_ids` — fetches summaries by id list

…the only missing piece was the meilisearch service in the prod
docker-compose. Adding it (and bumping the indexer's `BATCH_SIZE`
1000 + a 200 ms tight loop while backfilling so 18k messages
import in ~20 s instead of ~7 hours) closed the gap.

Post-deploy verification on prod (median of 2 runs):

| query | language | before (v1.4.27) | after (v1.4.29) | Δ |
|---|---|---:|---:|---:|
| `金額` | CJK (jp) | 597 ms | **40 ms** | **−93%** |
| `請求書` | CJK (jp) | (similar slow) | 35 ms | new |
| `ミーティング` | CJK (jp katakana) | (slow) | 20 ms | new |
| `発注` | CJK (jp) | (slow) | 37 ms | new |
| `invoice` | ASCII | 65 ms | **20 ms** | −69% |
| `meeting` | ASCII | 59 ms | 18 ms | −69% |

ASCII queries also benefit: meili's roundtrip is faster than the
6-branch BitmapOr CTE on PG. The PG path is still the fallback
when meili is unavailable (degraded mode), so search keeps working.

Topic fully closed.
