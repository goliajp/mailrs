# mailrs-imap-codec

[![Crates.io](https://img.shields.io/crates/v/mailrs-imap-codec.svg)](https://crates.io/crates/mailrs-imap-codec)
[![Docs.rs](https://docs.rs/mailrs-imap-codec/badge.svg)](https://docs.rs/mailrs-imap-codec)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Tokio `Decoder`/`Encoder` for the **RFC 9051 IMAP wire format** —
handles both line mode (CRLF-terminated commands and responses) and
literal mode (raw byte-counted payloads, used for `APPEND`,
`FETCH BODY[]`, passwords with special characters, etc.).

Pairs with [`mailrs-imap-proto`](https://crates.io/crates/mailrs-imap-proto):
this crate owns the **wire I/O** (line framing, literal handling),
`mailrs-imap-proto` owns the **command parsing** + state machine.

## Why

IMAP framing has two modes:

1. **Line mode** — typical commands and responses, CRLF-terminated:
   `A001 LOGIN alice secret\r\n` or `* OK ready\r\n`.
2. **Literal mode** — variable-length byte-counted payloads
   announced by `{N}\r\n` followed by exactly N bytes:
   ```text
   A002 APPEND INBOX {12}
   Hello world!
   A002 OK APPEND completed
   ```
   Literals can contain ANY bytes (including CRLF), so the codec
   must NOT split on CRLF while reading the literal.

`mailrs-imap-codec` handles the mode switching: the protocol layer
parses the `{N}` marker, calls
[`expect_literal(N)`](ImapCodec::expect_literal), and the codec
reads exactly N bytes then automatically returns to line mode.

## Quick start

```rust
use mailrs_imap_codec::{ImapCodec, ImapInput};
use tokio_util::codec::Decoder;
use bytes::BytesMut;

let mut codec = ImapCodec::new();

// Line mode: ordinary command
let mut buf = BytesMut::from("A001 SELECT INBOX\r\n".as_bytes());
match codec.decode(&mut buf).unwrap() {
    Some(ImapInput::Line(s)) => assert_eq!(s, "A001 SELECT INBOX"),
    _ => unreachable!(),
}

// After parsing `{12}` from the client, expect 12 bytes of literal:
codec.expect_literal(12);
let mut buf = BytesMut::from("Hello world!".as_bytes());
match codec.decode(&mut buf).unwrap() {
    Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"Hello world!"),
    _ => unreachable!(),
}
// Codec is back in line mode — the next decode parses a CRLF line.
```

## What this crate does NOT do

- No IMAP **command parsing** — that's `mailrs-imap-proto`.
- No IMAP **state machine** (auth / select / idle) — that's
  `mailrs-imap-proto::Session`.
- No **TLS** — that's `tokio-rustls` + your session layer.
- No **mailbox / message storage** — that's caller territory
  (or `mailrs-mailbox` / `mailrs-maildir`).

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-imap-codec`) |
| **test** | line cov: 92.9% (`cargo llvm-cov -p mailrs-imap-codec --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 1 gate(s) `perf_gate.rs` |
| **size** | release rlib: 27 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of **Apache-2.0** ([LICENSE-APACHE](./LICENSE-APACHE))
or **MIT** ([LICENSE-MIT](./LICENSE-MIT)) at your option.
