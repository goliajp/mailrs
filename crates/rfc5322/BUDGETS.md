# mailrs-rfc5322 performance budgets

Regression-catch budgets enforced by `tests/perf_gate.rs`. Each budget
is set with 15-30× headroom over the observed P95 on a dev machine so
CI fails on order-of-magnitude regressions, not micro-noise.

Run `cargo test -p mailrs-rfc5322 --test perf_gate` to check.
Run `cargo bench -p mailrs-rfc5322` for the full criterion baseline,
including the vs.-`mail-parser` comparison.

## Path taxonomy

RFC 5322 parsing is a **per-message warm** path — every inbound SMTP
DATA, every JMAP body fetch, every IMAP FETCH BODY[HEADER] uses it.
Per `rules/rust/patterns.md`, warm budgets sit ≤ 10 ms. The actual
work here is hundreds of nanoseconds.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `Message::header` (Subject + From, single message) | 10 µs | ~280 ns (release) / ~3 µs (dev) | ~3× |
| `Message::body` (first call, includes scan) | 10 µs | ~250 ns (release) | ~30× |
| `Message::body` (cached call, post-memo) | 1 µs | ~5 ns (release) | ~200× |
| `Message::header_all("Received")` (3 hops) | 20 µs | ~340 ns (release) | ~30× |

## Comparative numbers (criterion, vs mail-parser 0.11)

Real measured medians on M-series Mac, release profile, 100-sample.
**These are the numbers fit to quote** — see also workspace-level
[PERFORMANCE.md](../../PERFORMANCE.md).

| Operation | body size | mailrs-rfc5322 | mail-parser | speedup |
|---|---:|---:|---:|---:|
| header lookup (Subject + From) | 1 KB | 277 ns | 2383 ns | **8.6×** |
| header lookup (Subject + From) | 5 KB | 281 ns | 3378 ns | **12.0×** |
| header lookup (Subject + From) | 20 KB | 279 ns | 6901 ns | **24.7×** |
| body locate | 1 KB | 249 ns | 2387 ns | **9.6×** |
| body locate | 5 KB | 247 ns | 3337 ns | **13.5×** |
| body locate | 20 KB | 248 ns | 6855 ns | **27.6×** |
| received-chain walk (3 hops, 5 KB body) | — | 340 ns | 3382 ns | **9.9×** |

`mailrs-rfc5322` is **constant-time in body size** because the scanner
stops at the empty-line terminator separating headers from body.
`mail-parser` is **linear in body size** because it builds the full
Message tree on parse.

## Methodology

- Each test runs the path 100 times under criterion's harness.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time.

## When to re-measure

- Touching `find_unfolded_line_end` (the core scanner) or
  `Message::header` / `Message::body_offset`.
- After dependency bumps (we have none, so this section is
  forward-looking).
- CI runner class change.
