# mailrs-webhook-signature performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-webhook-signature --test perf_gate` to check.
Run `cargo bench -p mailrs-webhook-signature --bench signing` for
the full criterion baseline.

## Path taxonomy

`sign` runs **once per outbound webhook delivery**. `verify` runs
**once per inbound webhook request** plus during secret rotation.
Neither is a frame-budget path; sub-microsecond budgets are
documentation, not hard guards.

## Measured (criterion, M-series Mac, release, 100-sample median)

| Operation | Median |
|---|---:|
| `sign` (32-byte payload) | **~420 ns** |
| `sign` (1 KB payload) | **~1.6 µs** |
| `sign` (100 KB payload) | **~92 µs** |
| `verify` (correct path) | **~690 ns** |
| `verify` (wrong secret, constant-time) | **~650 ns** |
| `verify_any` (2 secrets, first matches) | **~700 ns** |
| `verify_any` (2 secrets, second matches) | **~915 ns** |
| `format_header` | **~36 ns** |
| `parse_header` (with prefix) | **~16 ns** |

The wrong-secret path takes ~the same time as the correct path —
that's the constant-time HMAC byte compare doing its job. An attacker
probing verify in a loop cannot recover the HMAC key via timing.

## Regression budgets

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `sign` (short) | 30 µs | ~5-10 µs | ~3-6× |
| `verify` (correct) | 30 µs | ~5-10 µs | ~3-6× |
| `format_header` | 5 µs | ~100-300 ns | ~15-50× |
| `parse_header` | 5 µs | ~50-200 ns | ~25-100× |

(`sign`/`verify` budgets are loose because HMAC in dev mode is
~25× slower than release.)

## Methodology

- Each test runs the path 200 times.
- Median sample asserted under the budget, not mean.
- Budgets are wall-clock, not CPU time.

## When to re-measure

- `hmac` or `sha2` major version bump.
- Switching HMAC implementation (e.g. to a SIMD-accelerated crate).
- `parse_header` adding more prefix variants.
