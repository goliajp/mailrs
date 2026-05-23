# mailrs-spf performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-spf --test perf_gate` to check.
Run `cargo bench -p mailrs-spf --bench spf` for the criterion
baseline.

## Path taxonomy

`verify` runs **once per inbound message** (warm). The parser is
the CPU piece; DNS is the wall-clock piece (5-50 ms per query, up to
10 queries per RFC 7208 §4.6.4 budget).

## Measured (criterion, M-series Mac, release)

| Operation | Median |
|---|---:|
| `Record::parse` simple | ~80 ns |
| `Record::parse` complex 8-mechanism | ~480 ns |
| `verify` pass path (no real DNS) | ~245 ns |

## Regression budgets

| Path | Budget | Observed P95 (dev) |
|---|---:|---:|
| `parse` simple | 5 µs | ~500ns-2µs |
| `parse` complex | 10 µs | ~1-3µs |

## Not in budget

- `verify` end-to-end — wall-clock DNS-bound (5-50 ms typical)
- DNS resolver — pluggable trait via `SpfResolver`
- Macro expansion (RFC 7208 §7) — not yet implemented; if added,
  budget separately

## When to re-measure

- Re-parsing tag-list with a different approach (currently `split_whitespace`)
- Adding macro expansion (deferred to 1.x minor)
- Switching IP subnet check (currently u128 bitwise)
