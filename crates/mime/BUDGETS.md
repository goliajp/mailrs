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

## Measured (criterion, M-series Mac, release; v4 ckpt 4, 2026-06-02)

Standalone parse paths:

| Operation | Median |
|---|---:|
| `parse` simple text/plain | **46 ns** |
| `parse` multipart/alternative (2 parts) | **317 ns** |
| `find_by_content_type("text/calendar")` (full parse + walk) | **611 ns** |

vs `mail-parser` 0.11 on the realistic invite shape (3-run median):

| Path | mailrs-mime | mail-parser | Winner |
|---|---:|---:|---|
| simple body_text | **86 ns** | 210 ns | **mailrs 2.4×** |
| invite, find text/calendar part | **619 ns** | 664 ns | **mailrs +7%** |

Transfer-encoding decoders (base64 4 KB input):

| Path | Median |
|---|---:|
| `decode_base64` clean (no WSP, fast-path) | ~2.5 µs |
| `decode_base64` wrapped (RFC 2045 76-col WSP, strip path) | ~6.5 µs |

The previous BUDGETS.md numbers (~170 ns simple / ~830 ns multipart /
~1.4 µs find_calendar) were pre-v4-round-13 — kept in git history.

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
