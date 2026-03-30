# Data Architecture

## Core Principle: Facts vs Derivations

ALL persistent data falls into one of two categories. Both may coexist in the database, but must be **explicitly distinguished** — by naming convention, schema separation, or column-level markers.

### Facts (immutable, append-only)

Real-world events that actually happened. Once recorded, never modified or deleted.

- **Input facts** — contractual/agreed values that feed into calculations (salary amount, commute distance, working days, tax rate effective dates)
- **Event facts** — things that occurred at a point in time (bank transfer executed, correction discovered, employee onboarded, rate table published)
- **Observation facts** — external data captured at a point in time (exchange rate, government-published tax bracket)

Properties:
- Immutable — if wrong, append a correction event, never UPDATE/DELETE the original
- Timestamped — every fact has `occurred_at` (when it happened) and `recorded_at` (when we learned about it)
- Self-contained — contains all context needed to understand it without joining to mutable state

### Derivations (recomputable, disposable)

Values calculated from facts. Can be regenerated at any time from the fact history.

- **Snapshots** — point-in-time computation results cached for performance (payslip, monthly P&L, tax withholding amount)
- **Aggregations** — sums, counts, averages over fact sets (monthly total, YTD balance)
- **Comparisons** — diffs between two snapshots (adjustment amount, variance)

Properties:
- Regenerable — deleting all snapshots and recomputing from facts must produce identical results
- Versioned — each recomputation creates a new version, old versions kept for audit
- Never authoritative — if a snapshot disagrees with a recomputation from facts, the recomputation wins

## Layering Rules

```
┌─────────────────────────────────┐
│  Presentation (UI / API)        │  ← reads snapshots for display
├─────────────────────────────────┤
│  Computation (business logic)   │  ← facts in → derivations out
├─────────────────────────────────┤
│  Fact Store (append-only)       │  ← writes go here, immutable
└─────────────────────────────────┘
```

1. **Writes always target the fact store** — UI actions create events, never update derived tables directly
2. **Computation is stateless** — given the same facts and rules, always produces the same output
3. **Snapshots are caches** — may be stale, always rebuildable, never the source of truth
4. **Reads prefer snapshots** — for performance, but can always fall back to recomputation

## Correction Pattern

When a past error is discovered:

```
WRONG:  UPDATE original_record SET field = correct_value
RIGHT:  INSERT correction_event (original_ref, field, old_value, new_value, reason, discovered_at)
        → recompute affected snapshots
        → diff old vs new snapshots = adjustment amount
```

The original record stays untouched. The correction event is a new fact. The adjustment amount is a derivation.

## Schema Design Checklist

Before creating or modifying a table, ask:

- [ ] Is every column either a fact or clearly marked as a cached derivation?
- [ ] If a row is wrong, can I fix it by appending a new event (not updating in place)?
- [ ] If I delete all derived/snapshot tables, can the system rebuild them from facts?
- [ ] Does the schema distinguish `occurred_at` (real-world time) from `recorded_at` (system time)?
- [ ] Are comparison values (diffs, adjustments) computed from snapshots rather than stored as independent facts?

## Coexistence: Why Derivations Belong in the DB Too

Derivations are **not second-class**. Storing them in the database is often necessary and correct:

- **Performance** — recomputing a full payslip on every page load is wasteful; a cached snapshot table avoids it
- **Downstream computation** — aggregations, reports, and cross-entity joins are far easier when pre-computed values are materialized
- **Audit trail** — versioned snapshots record "what the system believed at time T", valuable even if recomputable
- **API contracts** — external consumers may depend on stable, pre-computed values rather than raw facts

The rule is not "don't store derivations" — it is **know which is which**:

| | Fact | Derivation |
|--|------|------------|
| Purpose | record reality | serve computation / display |
| Mutability | append-only, never update | overwrite / version freely |
| Authority | source of truth | cache, rebuildable from facts |
| On conflict | facts win | recompute from facts |
| Naming hint | `_events`, `_inputs`, `_log` | `_snapshots`, `_cache`, `_computed` |

A table that mixes fact columns and derived columns in the same row is a design smell. If unavoidable, mark derived columns explicitly (e.g., `_computed` suffix, or a schema comment).

## Practical Exceptions

- **Draft/staging data** (not yet committed) may be mutable until finalized
- **User preferences and config** are mutable state, not facts — separate them from business data
- **Caching/materialized views** may be overwritten freely since they're derivations
