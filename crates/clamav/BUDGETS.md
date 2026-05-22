# mailrs-clamav performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-clamav --test perf_gate` to check.
Run `cargo bench -p mailrs-clamav --bench clamav` for the criterion
baseline.

## Path taxonomy

`parse_response` runs **once per scan**. CPU-bound, microseconds at
most. `scan` itself is network-bound — wall-clock dominated by the
DNS / TCP / clamd-scan time, not by our code.

The point of the perf gate here is to catch accidental regressions
in the parse path (e.g. switching to a regex implementation that's
10× slower under dev).

## Measured (criterion, M-series Mac, release, 100-sample median)

| Operation | Median |
|---|---:|
| `parse_response` (clean reply) | ~25 ns |
| `parse_response` (clean with trailing NUL) | ~30 ns |
| `parse_response` (virus, short name) | ~70 ns |
| `parse_response` (virus, long name with dots/dashes) | ~75 ns |
| `parse_response` (error string) | ~95 ns |
| `parse_response` (empty input) | ~10 ns |

## Regression budgets

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `parse_response` clean | 2 µs | ~200-500 ns | ~5-10× |
| `parse_response` virus | 5 µs | ~500ns-2µs | ~3-10× |
| `parse_response` error | 5 µs | ~500ns-2µs | ~3-10× |

## What is NOT in this budget

- `scan` end-to-end — network-bound, see clamd performance docs
- `ping` / `version` — network-bound

## When to re-measure

- Switching `parse_response` from byte-search to regex.
- Adding more wire-format variants to the parser.
