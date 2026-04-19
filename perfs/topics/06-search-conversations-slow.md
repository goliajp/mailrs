# Topic 06: `/api/conversations/search` ~600 ms TTFB

**Status:** open
**Severity:** high (search is a primary user surface)
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

- 2026-04-19 — measured + read the SQL. EXPLAIN not yet captured.

## Decision

—

## Verification

—
