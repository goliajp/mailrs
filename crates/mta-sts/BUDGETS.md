# Perf budgets — mailrs-mta-sts

Per-test budgets enforced by `tests/perf_gate.rs`. Two-tier: release
(`cargo bench`-grade) and debug (`cargo test` from `release.sh`). Derived
from observed P95 on Apple M-class silicon plus headroom (~10× release,
~5× more on top for debug).

| Test                        | Release P95 | Release budget | Debug budget |
|-----------------------------|-------------|----------------|--------------|
| `parse_sts_record`          |  78 ns      | 1 µs           | 5 µs         |
| `parse_policy` (6-line)     | 321 ns      | 5 µs           | 25 µs        |
| `mx_matches` wildcard       | 100 ns      | 1 µs           | 5 µs         |
| `enforce` 3-mx last match   | 223 ns      | 3 µs           | 15 µs        |

## Rules

- **Never weaken without re-measuring P95** and naming the regression in the
  commit message.
- **Never skip perf tests "to save CI time."** They run with the unit suite.
- **Budgets are floors of pain, not targets.** The crate aims for the
  numbers in the README; budgets exist to catch order-of-magnitude regressions.
