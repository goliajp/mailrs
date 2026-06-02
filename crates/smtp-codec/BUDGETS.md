# mailrs-smtp-codec performance budgets

Regression budgets enforced by `tests/perf_gate.rs`.
Run `cargo test -p mailrs-smtp-codec --test perf_gate` to check.
Run `cargo bench -p mailrs-smtp-codec` for criterion baselines.

## Path taxonomy

All public ops are sub-millisecond on M-series Mac. Budgets sit at
~10-50× the observed median to catch order-of-magnitude regressions
without flaking under cargo-test-workspace parallel CPU contention.

## Measured (release, M-series Mac, v4 round 1 — 2026-06-02)

| Path | Median | Budget (`perf_gate.rs`) | Headroom |
|---|---:|---:|---:|
| `has_smuggle_sequence/clean_1024b` | 12.7 ns | 10 µs (existing gate) | ~800× |
| `has_smuggle_sequence/clean_10240b` | 95 ns | — | — |
| `has_smuggle_sequence/clean_102400b` | 907 ns | — | — |
| `normalize_line_endings/bare_lf_1024b` | 152 ns | 100 µs (new gate) | ~660× |
| `normalize_line_endings/bare_lf_10240b` | 3.56 µs | — | — |
| `normalize_line_endings/bare_lf_102400b` | 18.8 µs | — | — |
| `decode/command/ehlo` | 78 ns | — | — |
| `decode/data/permissive_102400b` | 52.1 µs | — | — |

Budgets are intentionally loose (~700× headroom) — they catch
order-of-magnitude regressions (memchr removal, accidental quadratic
loop), not micro-tuning. For tighter tracking, use the criterion
historical comparison printed by `cargo bench`.

Full per-op table + cross-input comparison in workspace
[`PERFORMANCE.md`](../../PERFORMANCE.md) "`mailrs-smtp-codec`" section.

## How to add a new gate

1. Pick an op that's on a real hot path
2. Add a `#[test]` with `time_median` measuring it
3. Pick budget at 15-30× over the observed P95
4. Document the budget here with reasoning
