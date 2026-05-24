# mailrs-arf performance budgets

Regression budgets enforced by `tests/perf_gate.rs`. Run
`cargo test -p mailrs-arf --test perf_gate` to check.
Run `cargo bench -p mailrs-arf --bench arf` for criterion baselines.

## Path taxonomy

`parse` runs **once per incoming FBL message at the abuse@ mailbox**.
A high-volume sender receives at most a few hundred FBL reports per
day; this is not a frame-budget hot path. Budgets are set so a clear
order-of-magnitude regression triggers CI failure.

## Budgets

| Operation | Budget | Observed (release) |
|---|---:|---:|
| `parse(hotmail_sample)` | < 30 µs | ~2 µs |
| `parse(non_arf_input)` | < 5 µs | < 200 ns |

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

## What's measured but not gated

Criterion benches (`cargo bench`) produce more detailed numbers
(median + IQR + outlier analysis) — useful for tracking trends across
versions but not run in CI.
