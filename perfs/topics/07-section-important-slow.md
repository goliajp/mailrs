# Topic 07: `?section=important` 581 ms total (slowest list variant)

**Status:** open
**Severity:** medium
**First observed:** 2026-04-19 (data/2026-04-19/sweep.txt)
**Owner:** —

## Symptom

Same `/api/conversations` endpoint, three section variants:

| variant | size | TTFB | total |
|---|---:|---:|---:|
| `?section=action` | 32.2 KB | 255 ms | 292 ms |
| **`?section=important`** | 15.7 KB | **376 ms** | **581 ms** |
| `?section=other` | (not measured) | — | — |
| baseline (no section) | 36.1 KB | 270 ms | 308 ms |

`section=important` is **slower than the unfiltered list** despite returning *less* data, and slower than `section=action`. That's a HAVING-clause cost, not a row-count cost.

## Reproduction

```bash
TOKEN=… ./scripts/timing.sh "section=important" GET 'https://mail.golia.ai/api/conversations?limit=50&section=important'
TOKEN=… ./scripts/timing.sh "section=action"    GET 'https://mail.golia.ai/api/conversations?limit=50&section=action'
TOKEN=… ./scripts/timing.sh "section=other"     GET 'https://mail.golia.ai/api/conversations?limit=50&section=other'
```

## Hypotheses

`crates/mailbox/src/store.rs:873-883` adds this HAVING clause for `important`:

```sql
COALESCE((SELECT m_imp.importance_level
          FROM messages m_imp
          WHERE m_imp.thread_id = m.thread_id
          ORDER BY m_imp.importance_score DESC NULLS LAST LIMIT 1),
         'normal') IN ('critical', 'important')
```

This is a per-group SubPlan exactly like the `hide_my_latest` one fixed in topic-01 fix-a — it runs once per group before LIMIT. **`section=important` and `section=other` both pay it; `section=action` uses a different (cheaper) BOOL_OR pattern, which explains why action is fast and important/other are slow.**

The data weight is also informative: `important` returns 15.7 KB vs `action` 32.2 KB, so `important` filters *more* threads out, yet runs longer — confirming the filter cost is in the HAVING SubPlan, not in the rows we keep.

## Fix candidates

| | change | expected win | risk |
|---|---|---|---|
| **a** | Same trick as topic-01 fix-a: replace the per-group SubPlan with an ordered aggregate over `importance_level` keyed by `importance_score`. The SELECT list already has `MAX(m.importance_score)`; for the level we need an ordered aggregate. | proportional to the SubPlan loops — likely ~250 ms. | low: same SQL pattern as the proven fix. |
| **b** | Persist `thread_summary.importance_level` (snapshot table per topic-01 fix-d) and read it directly. | brings to <50 ms. | high (cross-cutting). |

Recommendation: **a**, same shape as topic-01 fix-a, single-line change in the HAVING builder.

## Investigation log

- 2026-04-19 — measured + read the SQL. EXPLAIN not yet captured.

## Decision

—

## Verification

—
