# mailrs-rfc2231 performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-rfc2231 --test perf_gate` to check.
Run `cargo bench -p mailrs-rfc2231 --bench params` for the full
criterion baseline.

## Path taxonomy

`encode_param` runs on **outbound message construction** — once per
non-trivial attachment header, occasionally never. `decode_param_value`
runs on **inbound MIME parse** — typical inbound has 0-3 calls
(Content-Type charset + Content-Disposition filename).

Neither is a hot frame-budget path; sub-microsecond budgets are
documentation, not hard guards.

## Measured (criterion, M-series Mac, release, 100-sample median)

| Path | Median |
|---|---:|
| `encode_param` (ASCII, legacy quoted) | ~25 ns |
| `encode_param` (Japanese, extended) | ~140 ns |
| `encode_param` (60-char Japanese filename) | ~350 ns |
| `decode_param_value` (legacy quoted) | ~15 ns |
| `decode_param_value` (legacy bareword) | ~5 ns |
| `decode_param_value` (UTF-8 extended) | ~95 ns |
| `decode_param_value` (ISO-8859-1 extended) | ~90 ns |

## Regression budgets

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `encode_param` ASCII | 5 µs | ~100-500 ns | ~10-50× |
| `encode_param` Japanese | 10 µs | ~500ns-2 µs | ~5-20× |
| `decode_param_value` quoted | 5 µs | ~50-300 ns | ~15-100× |
| `decode_param_value` extended UTF-8 | 10 µs | ~500ns-2µs | ~5-20× |

## When to re-measure

- ASCII_HEX_UPPER table replaced with `{:02X}` format (it would be
  ~3× slower; documented here to prevent reverting).
- encoding_rs major bump.
- Switching `is_param_safe` byte test to a `[bool; 256]` table.
