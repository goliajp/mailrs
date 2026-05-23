# mailrs-mime performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-mime --test perf_gate` to check.
Run `cargo bench -p mailrs-mime --bench mime` for the criterion
baseline + comparison vs `mail-parser`.

## Path taxonomy

`parse` runs **once per inbound message** (warm path) when the
inbound pipeline needs the MIME tree (body text + attachment
extraction + calendar invite finding).

Network / disk dominate inbound wall-clock time; the parser is the
CPU piece.

## Measured (criterion, M-series Mac, release)

| Operation | Median |
|---|---:|
| `parse` simple text/plain | ~170 ns |
| `parse` multipart/alternative (2 parts) | ~830 ns |
| `find_by_content_type("text/calendar")` (full parse + walk) | ~1.4 µs |

vs `mail-parser` 0.11:

| Path | mailrs-mime | mail-parser |
|---|---:|---:|
| simple body_text | 207 ns | 194 ns |
| invite-shape, first part lookup | 1.38 µs | 630 ns |

## Regression budgets

| Path | Budget | Observed P95 (dev) |
|---|---:|---:|
| `parse` simple | 5 µs | ~500ns-2µs |
| `parse` multipart | 10 µs | ~2-5µs |
| find calendar | 20 µs | ~5-10µs |

## Not in budget

- I/O — caller-owned (this crate operates on `&[u8]`)
- Charset conversion — `encoding_rs` handles, no useful budget here
- Base64 / quoted-printable — bounded by input size, no fixed budget

## When to re-measure

- Switching `encoding_rs` major version
- Reworking the boundary scanner (e.g. memchr SIMD)
- Adding RFC 2047 header decode pre-pass to `parse`
