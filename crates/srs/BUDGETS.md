# mailrs-srs performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-srs --test perf_gate` to check.
Run `cargo bench -p mailrs-srs --bench srs` for criterion baselines.

## Path taxonomy

`rewrite` runs **once per outbound forwarded message**. `reverse` runs
**once per inbound bounce** plus possibly per inbound DSN. Both are
sub-microsecond — neither should ever be the bottleneck in an SMTP
relay pipeline.

## Measured (criterion, M-series Mac, release, 100-sample median)

| Operation | Median |
|---|---:|
| `rewrite` (ASCII sender) | ~270 ns |
| `reverse` (success path, in window) | ~290 ns |
| `reverse` (wrong-secret, constant-time) | ~280 ns |
| `reverse` (malformed input, early exit) | < 100 ns |

The success and wrong-secret paths take nearly identical time — that's
the constant-time HMAC compare doing its job. An attacker probing
`reverse()` in a loop with crafted inputs cannot recover the HMAC key
by timing analysis.

## Regression budgets

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `rewrite` | 5 µs | ~500 ns - 2 µs | ~10-25× |
| `reverse` (success) | 5 µs | ~500 ns - 2 µs | ~10-25× |
| `reverse` (wrong secret) | 5 µs | ~500 ns - 2 µs | ~10-25× |
| `reverse` (malformed) | 1 µs | ~50-200 ns | ~5-20× |

## Methodology

- Each test runs the path 200 times.
- Median sample asserted under the budget, not mean.
- Budgets are wall-clock, not CPU time.

## When to re-measure

- HMAC implementation switching (e.g. SHA256 → BLAKE3).
- Constant-time compare implementation changing.
- `HASH_HEX_LEN` constant changing (currently 8).
