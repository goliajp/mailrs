# mailrs-imap-codec performance budgets

Regression budgets enforced by `tests/perf_gate.rs`.
Run `cargo test -p mailrs-imap-codec --test perf_gate` to check.
Run `cargo bench -p mailrs-imap-codec` for criterion baselines.

## Path taxonomy

All public ops are sub-millisecond on M-series Mac. Budgets sit at
~10-50× the observed median to catch order-of-magnitude regressions
without flaking under cargo-test-workspace parallel CPU contention.

## Measured (release, M-series Mac, v4 round 1 — 2026-06-02)

| Path | Median | Budget (`perf_gate.rs`) | Headroom |
|---|---:|---:|---:|
| `decode/line/login` (22 B) | 72 ns | 10 µs (existing gate) | ~140× |
| `decode/line/noop` (11 B) | 65 ns | — | — |
| `decode/line/fetch_long` (160 B) | 107 ns | — | — |
| `decode/line/bare_cr_skip` (24 B, 5 CRs) | 76 ns | — | — |
| `decode/literal/32b` | 62 ns | — | — |
| `decode/literal/1024b` | 87.5 ns | — | — |
| `decode/literal/102400b` | 13.2 µs | 100 µs (new gate) | ~7.5× |
| `encode/short_12b` | 38 ns | — | — |
| `encode/long_140b` | 39.4 ns | — | — |

v4 ckpt 2 = **Case A** — line scanner already memchr-anchored
(v3 cycle), literal path is `BytesMut::split_to` + `to_vec`
(memcpy bound). No exploitable hot path; numbers sit within
~30 % of the hardware floor.

The 100 KB literal-decode gate has only ~7.5× headroom (tighter
than the line gate's ~140×). That's intentional — literal decode
is memcpy-bound, so the budget catches an algorithmic regression
(e.g. accidental copy duplication) but does not absorb thermal /
parallel-test noise comfortably. If the gate ever flakes under
`cargo test --workspace`, loosen the budget rather than dilute
the meaning of "regression".

Full per-op table + label rationale in workspace
[`PERFORMANCE.md`](../../PERFORMANCE.md) "`mailrs-imap-codec`" section.

## How to add a new gate

1. Pick an op that's on a real hot path
2. Add a `#[test]` with `time_median` measuring it
3. Pick budget at 15-30× over the observed P95
4. Document the budget here with reasoning
