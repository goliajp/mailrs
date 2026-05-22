# mailrs-rfc2047 performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-rfc2047 --test perf_gate` to check.
Run `cargo bench -p mailrs-rfc2047 --bench decode` for the comparative
numbers vs `mail-parser`.

## Path taxonomy

Encoded-word decoding is a **per-message warm** path. Every Subject
+ every From display name from non-Latin senders goes through here.
Per `rules/rust/patterns.md`: warm budgets sit ≤ 10 ms; actual work
is tens to a hundred nanoseconds.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `decode` (ASCII passthrough — fast-path) | 1 µs | ~25 ns (release) | ~30× |
| `decode` (UTF-8 Base64 short subject) | 1 µs | ~66 ns (release) | ~15× |
| `decode` (UTF-8 Q short subject) | 1 µs | ~78 ns (release) | ~13× |
| `decode` (ISO-2022-JP short subject) | 5 µs | ~154 ns (release) | ~30× |
| `decode` (mixed ASCII + encoded) | 1 µs | ~104 ns (release) | ~10× |

## Comparative numbers vs `mail-parser` 0.11

(criterion, M-series Mac, release, 100-sample median)

| Operation | mail-parser | mailrs-rfc2047 | speedup |
|---|---:|---:|---:|
| Subject extraction (ASCII) | 442 ns | **28 ns** | **15.8×** |
| Subject extraction (UTF-8 Base64) | 439 ns | **110 ns** | **4.0×** |

Note `mail-parser` builds the full Message tree even when only Subject
is read. Decoder-only comparison would be tighter — but real-world
callers go through mail-parser's full parse because there's no
narrower API. `mailrs-rfc2047` paired with `mailrs-rfc5322::Message::header`
gets you the decoded value at a fraction of the cost.

## Methodology

- Each test runs the path 100 times under criterion's harness.
- Median sample is asserted under the budget, not mean.
- Budgets are wall-clock, not CPU time.

## When to re-measure

- Touching the encoded-word scanner (`find_encoded_word_*`,
  `decode_q`, `convert_to_utf8`).
- `encoding_rs` major version bump.
