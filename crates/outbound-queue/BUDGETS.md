# mailrs-outbound-queue performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-outbound-queue --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `retry_delay_secs` (10-attempt sequence) | 10 µs | ~100 ns | ~100× |
| `should_bounce` (10-attempt sequence) | 10 µs | ~50 ns | ~200× |

## DKIM signing is NOT gated here

RSA signing in debug mode is 50-100× slower than release, which would
make a wall-clock budget either useless ("3 sec is fine") or force
us to run perf_gate in `--release` only — neither integrates cleanly
with the standard `cargo test --workspace` flow.

The criterion bench at `benches/core.rs::bench_dkim_sign` covers this
path; rely on that for DKIM regression tracking.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
