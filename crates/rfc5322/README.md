# mailrs-rfc5322

[![Crates.io](https://img.shields.io/crates/v/mailrs-rfc5322?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-rfc5322)
[![docs.rs](https://img.shields.io/docsrs/mailrs-rfc5322?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-rfc5322)
[![License](https://img.shields.io/crates/l/mailrs-rfc5322?style=flat-square)](#license)

Pull-based RFC 5322 message parser. **Lazy header lookup, byte-level
scan, zero-allocation borrowed slices.** Skip-ahead to the header you
want without building the full Message tree.

The crate exists because building a full typed Message tree on every
inbound SMTP receive is wasted work when the receiver only needs 1-5
specific headers (Subject, From, Message-ID, Authentication-Results, …).
This parser scans for the header you ask for and stops; the rest of
the message is never touched.

## Quickstart

```rust
use mailrs_rfc5322::Message;

let raw = b"\
From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: hi\r\n\
\r\n\
Hello, world!\r\n";

let msg = Message::new(raw);
assert_eq!(msg.header_str("Subject"), Some("hi"));
assert_eq!(msg.header("From"), Some(b"alice@example.com" as &[u8]));
assert_eq!(msg.body(), Some(b"Hello, world!\r\n" as &[u8]));

// Multiple Received: chains
for received in msg.header_all("Received") {
    println!("hop: {:?}", received.value);
}
```

## When to reach for this vs. mail-parser

| Use case | Pick |
|---|---|
| Read 1-5 specific headers per inbound message | **mailrs-rfc5322** |
| Decide if a message has a `text/plain` part | **mailrs-rfc5322** + downstream MIME |
| Display a full message tree with all MIME parts decoded, all RFC 2047 encoded-words resolved, all addresses parsed into structured `(name, email)` tuples | `mail-parser` |
| Verify SPF/DKIM/DMARC (reads message bytes + a few headers) | **mailrs-rfc5322** |

`mailrs-rfc5322` is intentionally minimal. It does **not** decode
RFC 2047 encoded-words, **not** parse MIME bodies into a tree, **not**
extract `(display-name, email)` from address fields. It hands you the
raw header value bytes and lets you decide what to do.

## What this crate does

- Header lookup by name, case-insensitive (`Message::header`,
  `Message::header_str`, `Message::header_all`)
- Iteration over all headers in document order
  (`Message::headers`)
- Body access (`Message::body`) — bytes after the header
  terminator, memoized
- RFC 5322 §3.2.2 line folding handled (continuation lines
  starting with whitespace are kept inside the same header value)
- LF-only and CRLF line endings both accepted
- Zero allocation. Every returned `&[u8]` / `&str` borrows from
  the input message bytes.
- Zero dependencies. The crate compiles to a small object that
  pulls in nothing transitive.

## What this crate does not do (and won't, in 1.x)

- RFC 2047 encoded-word decoding (`=?utf-8?B?...?=` in header
  values). Use a downstream crate.
- MIME body parsing (`multipart/`, `Content-Transfer-Encoding`).
  Use `mail-parser` or a focused MIME crate.
- Address field structured parse (`From: "Name" <addr@example>`).
  Hand the value to a focused crate.
- RFC 6532 UTF-8-in-headers validation. Bytes are returned raw;
  the `_str` helpers do a UTF-8 check but don't do canonical
  comparison.

If a 1.x version "added MIME parsing" it would defeat the purpose.
Future MIME work goes in a separate crate.

## Performance

**Measured** (criterion, M-series Mac, release, 100-sample median):

| Operation | body size | mailrs-rfc5322 | mail-parser 0.11 | speedup |
|---|---:|---:|---:|---:|
| Subject + From lookup | 1 KB | **212 ns** | 2383 ns | **11.2×** |
| Subject + From lookup | 5 KB | **212 ns** | 3378 ns | **15.9×** |
| Subject + From lookup | 20 KB | **212 ns** | 6901 ns | **32.5×** |
| Target at end of 50 headers (worst case) | — | **393 ns** | n/a | n/a |
| body offset locate | 1 KB | **249 ns** | 2387 ns | **9.6×** |
| body offset locate | 5 KB | **247 ns** | 3337 ns | **13.5×** |
| body offset locate | 20 KB | **248 ns** | 6855 ns | **27.6×** |
| Received-chain walk (3 hops, 5 KB body) | — | **340 ns** | 3382 ns | **9.9×** |

Note the **mailrs-rfc5322 numbers are constant in body size** —
~280 ns for header lookup regardless of whether the body is 1 KB or
20 KB. That's because the scanner stops at the empty-line terminator
separating headers from body. `mail-parser` builds the full Message
tree on every parse, so it's linear in body size.

Reproduce with `cargo bench -p mailrs-rfc5322 --bench parse`. Workspace
[PERFORMANCE.md](../../PERFORMANCE.md) carries the same table; per the
project's "no fake numbers" rule, every number traces to a measurement.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-rfc5322`) |
| **test** | line cov: 96.7% (`cargo llvm-cov -p mailrs-rfc5322 --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 4 gate(s) `perf_gate.rs` |
| **size** | release rlib: 43 KB |
| **fuzz** | ✅ 1 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons (from PERFORMANCE.md)

- `mailrs-rfc5322` vs `mail-parser` (header lookup, lazy)
- `mailrs-rfc5322` vs `mail-parser` — comparative bench

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
