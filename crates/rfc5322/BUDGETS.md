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

| Path | Budget | Observed P95 (release, v4 round 1) | Headroom |
|---|---:|---:|---:|
| `Message::header` (Subject + From, single message) | 10 µs | **~85 ns** | ~120× |
| `Message::body` (first call, includes scan) | 10 µs | **~105 ns** | ~95× |
| `Message::body` (cached call, post-memo) | 1 µs | ~650 ps | ~1500× |
| `Message::header_all("Received")` (3 hops) | 20 µs | **~127 ns** | ~160× |

## Comparative numbers (criterion, vs mail-parser 0.11)

Real measured medians on M-series Mac, release profile, 100-sample.
**These are the numbers fit to quote** — see also workspace-level
[PERFORMANCE.md](../../PERFORMANCE.md).

| Operation | body size | mailrs-rfc5322 | mail-parser | speedup |
|---|---:|---:|---:|---:|
| header lookup (Subject + From) | 1 KB | **83 ns** | 2629 ns | **31.7×** |
| header lookup (Subject + From) | 5 KB | **84 ns** | 3727 ns | **44.4×** |
| header lookup (Subject + From) | 20 KB | **84 ns** | 7682 ns | **91.5×** |
| body locate | 1 KB | **104 ns** | 2554 ns | **24.6×** |
| body locate | 5 KB | **105 ns** | 3654 ns | **34.7×** |
| body locate | 20 KB | **105 ns** | 7674 ns | **73.0×** |
| received-chain walk (3 hops, 5 KB body) | — | **127 ns** | 3691 ns | **29.1×** |

**v4 round 1** (2026-06-02): swapped two `iter().position()` byte-by-byte
scans in `header.rs` (LF in `find_unfolded_line_end`, colon in
`parse_header_line`) for `memchr::memchr`. Header lookup dropped from
222 ns → 84 ns (−62 % / **2.6×**); the speedup vs mail-parser tripled
(11-33× → 31-91×).

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
