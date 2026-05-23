# Perf budgets — mailrs-tls-rpt

Per-test budgets enforced by `tests/perf_gate.rs`. Two-tier: release
(`cargo bench`-grade) and debug (`cargo test` from `release.sh`).
~10× release P95 + ~5× more on top for debug.

| Test                                | Release P95 | Release budget | Debug budget |
|-------------------------------------|-------------|----------------|--------------|
| `parse_record_single`               | 164 ns      | 2 µs           | 10 µs        |
| `parse_record_multi` (3 rua)        | 280 ns      | 3 µs           | 15 µs        |
| `build_100_success`                 |  2.7 µs     | 30 µs          | 150 µs       |
| `serialize_100_success`             | 750 ns      | 8 µs           | 40 µs        |

## Rules

- **Never weaken without re-measuring P95** and naming the regression
  in the commit message.
- **Never skip perf tests "to save CI time."** They run with the
  unit suite.
- **Budgets are floors of pain, not targets.** The crate aims for the
  numbers in the README; budgets exist to catch order-of-magnitude
  regressions.
