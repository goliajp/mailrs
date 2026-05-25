# mailrs-attachment-extract performance budgets

Regression budgets enforced by `tests/perf_gate.rs`.
Run `cargo test -p mailrs-attachment-extract --test perf_gate` to check.
Run `cargo bench -p mailrs-attachment-extract` for criterion baselines.

## Path taxonomy

All public ops are sub-millisecond on M-series Mac. Budgets sit at
~10-50× the observed median to catch order-of-magnitude regressions
without flaking under cargo-test-workspace parallel CPU contention.

## Measured (release, M-series Mac)

Numbers populated by `scripts/stone-audit-footer.py` from criterion
runs — see README.md "Stone audit" footer for current snapshot.

## How to add a new gate

1. Pick an op that's on a real hot path
2. Add a `#[test]` with `time_median` measuring it
3. Pick budget at 15-30× over the observed P95
4. Document the budget here with reasoning
