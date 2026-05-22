# mailrs-auth-guard performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-auth-guard --test perf_gate` to check.
Run `cargo bench -p mailrs-auth-guard --bench guard` for the full
criterion baseline.

## Path taxonomy

`check` is a **per-auth-attempt warm** path. Every legitimate login
+ every brute-force probe runs through `check` exactly once before
the actual password verification. Sub-microsecond budgets reflect
this: the guard should never be the bottleneck.

## Measured (criterion, M-series Mac, release, 100-sample median)

| Operation | Median |
|---|---:|
| `check` — empty map (success path) | **43 ns** |
| `check` — below threshold, still allowed | **46 ns** |
| `check` — IP locked out | **51 ns** |
| `record_failure` — fresh (IP, username) key | **127 ns** |
| `record_failure` — repeat same key | **75 ns** |
| `record_success` — clears account counter | **62 ns** |

## Regression budgets

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `check` — empty map | 1 µs | ~50-200 ns | ~10× |
| `check` — locked out | 2 µs | ~100-300 ns | ~10× |
| `record_failure` — repeat | 10 µs | ~200-500 ns | ~20× |
| `cleanup_stale` — 1k entries | 5 ms | ~100-500 µs | ~10× |

## Methodology

- Each test runs the path 100 times under criterion's harness.
- Median sample asserted under the budget, not mean.
- Budgets are wall-clock, not CPU time.

## When to re-measure

- Touching the DashMap shard count default in newer dashmap versions.
- IPv6 normalization changing (e.g. switching to /48 prefix).
- New `AuthGuardConfig` defaults if they change `*_window_secs`.
