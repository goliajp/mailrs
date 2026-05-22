# mailrs-backoff performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-backoff --test perf_gate` to check.
Run `cargo bench -p mailrs-backoff --bench backoff` for the criterion
baseline.

## Path taxonomy

`Backoff::delay` runs **once per retry attempt** in a typical retry
loop — rare in absolute terms, but `mailrs-backoff` is meant to be
"essentially free" relative to anything else the loop does
(network I/O, DB writes, etc).

All paths are pure math (no syscalls, no allocation, no async),
single-digit nanoseconds in release.

## Measured (criterion, M-series Mac, release, 100-sample median)

| Operation | Median |
|---|---:|
| `base_delay(attempt=3)` | ~1.2 ns |
| `delay(attempt=3, Jitter::None)` | ~1.5 ns |
| `delay(attempt=3, Jitter::Equal)` | ~3.5 ns |
| `delay(attempt=3, Jitter::Full)` | ~3.0 ns |
| `should_give_up` | <1 ns |
| `delay(attempt=100, capped)` | ~3.5 ns |

## Regression budgets

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `base_delay` | 1 µs | ~50-200 ns | ~5-20× |
| `delay/none` | 1 µs | ~50-300 ns | ~3-20× |
| `delay/full` | 1 µs | ~100-500 ns | ~2-10× |
| `delay/high_attempt` | 1 µs | ~100-500 ns | ~2-10× |

## When to re-measure

- Switching SplitMix64 jitter to a different RNG step.
- Adding a "decorrelated jitter" variant (would need attempt-N state).
- Replacing `multiplier.powi(attempt as i32)` with table lookup for
  common attempt counts.
