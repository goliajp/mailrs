# mailrs-outbound-queue performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-outbound-queue --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `retry_delay_secs` (10-attempt sequence) | 10 µs | ~100 ns | ~100× |
| `should_bounce` (10-attempt sequence) | 10 µs | ~50 ns | ~200× |
| `mx_matches_policy` exact (×100) | 300 µs | ~14 µs | ~20× |
| `mx_matches_policy` wildcard, 5-pattern policy | 30 µs | ~1.5 µs | ~20× |
| `is_hard_bounce` (×100) | 200 µs | ~9 µs | ~22× |
| `format_dsn` | 400 µs | ~13 µs | ~30× |

The MTA-STS matcher runs once per outbound delivery (post cache-miss); the
wildcard shape is the realistic worst case for Google/Microsoft-style
multi-pattern policies. `is_hard_bounce` is the per-failed-delivery
classifier that decides Bounced vs Failed. `format_dsn` is the
per-bounce DSN body builder (multipart RFC 3464). All three are
order-of-magnitude gates — sized to catch accidental allocations,
regex compiles, or sync I/O on the hot path, not to track micro-jitter.

The `*_x100` variants batch the per-call work 100× per sample because a
single call sits below the timer floor (<100 ns).

## DKIM signing is NOT gated here

RSA signing in debug mode is 50-100× slower than release, which would
make a wall-clock budget either useless ("3 sec is fine") or force
us to run perf_gate in `--release` only — neither integrates cleanly
with the standard `cargo test --workspace` flow.

The criterion bench at `benches/core.rs::bench_dkim_sign` covers this
path; rely on that for DKIM regression tracking.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
