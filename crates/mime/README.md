# mailrs-mime

[![Crates.io](https://img.shields.io/crates/v/mailrs-mime?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-mime)
[![docs.rs](https://img.shields.io/docsrs/mailrs-mime?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-mime)
[![License](https://img.shields.io/crates/l/mailrs-mime?style=flat-square)](#license)

RFC 2045 / 2046 MIME body parser. Walks the multipart tree, decodes
Content-Transfer-Encoding (`base64`, `quoted-printable`, 7bit, 8bit,
binary), exposes text / HTML extraction + find-by-content-type +
attachment iteration.

**Layered**: built on top of
[`mailrs-rfc5322`](https://crates.io/crates/mailrs-rfc5322) (headers),
[`mailrs-rfc2231`](https://crates.io/crates/mailrs-rfc2231) (params),
and [`mailrs-rfc2047`](https://crates.io/crates/mailrs-rfc2047) is
available for downstream callers who need encoded-word header decode.

Replaces residual `mail-parser` usage in inbound code paths where
only the MIME tree shape matters (find calendar invite part, extract
attachments, get the text body for indexing).

## Quickstart

```rust
use mailrs_mime::parse;

let raw = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
\r\n\
--x\r\n\
Content-Type: text/plain\r\n\
\r\n\
plain version\r\n\
--x\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>html version</p>\r\n\
--x--\r\n";

let root = parse(raw);
assert!(root.content_type.is_multipart());

// Find the HTML alternative:
let html = root.find_by_content_type("text/html").unwrap();
assert_eq!(html.body_text().as_deref(), Some("<p>html version</p>"));

// Iterate all parts depth-first:
for part in root.walk() {
    println!("{}", part.content_type.mime_type());
}
```

## What this crate does

- **`parse(raw)` → `Part`** — top-level entry. Returns a tree
  matching the message's multipart structure.
- **`Part::walk()`** — depth-first iterator over self + descendants
- **`Part::find_by_content_type("text/calendar")`** — exact-match
  lookup, useful for iTIP / iMIP invite extraction
- **`Part::body_text()`** — for `text/*` leaves, decodes per the
  part's `charset=` (via `encoding_rs`)
- **`Part::attachments()` + `Part::attachment_filename()`** —
  finds parts marked attachment (by Content-Disposition or filename
  parameter)
- **Content-Transfer-Encoding decoders**: `base64`,
  `quoted-printable`, identity (7bit/8bit/binary), unknown
  (passes through)
- **RFC 2231 parameter decoding** built in — `filename*=UTF-8''…`
  forms surface as plain UTF-8 via `Part::attachment_filename()`

## What this crate does not

- **No RFC 2047 encoded-word decode in headers** — that's
  [`mailrs-rfc2047`](https://crates.io/crates/mailrs-rfc2047).
  Use it on `From` / `Subject` / display names after extracting
  via mailrs-rfc5322.
- **No body autodetection beyond charset** — if the message lies
  about its charset, we trust the lie. (Real parsers occasionally
  apply heuristics for mojibake recovery; out of v1.0 scope.)
- **No DKIM / SPF / DMARC** — separate stones
  (`mailrs-dkim`, `mailrs-spf`, `mailrs-dmarc`).
- **No address-list structured parse** — `From:` returns raw bytes
  via mailrs-rfc5322; building `(name, addr)` from it is downstream.
- **No outbound MIME builder** — this crate is read-only. Use a
  builder crate for composing outbound mail.

## Layering

```text
mailrs-mime                       ← this crate (MIME body tree)
   ├── mailrs-rfc5322             ← header lookup, lazy
   ├── mailrs-rfc2231             ← parameter encode/decode
   └── encoding_rs + base64       ← foundational
```

Each layer is independently published; downstream callers can pull
whichever subset they need without the rest.

## Performance

Measured (criterion, M-series Mac, release):

| Operation | Median |
|---|---:|
| `parse` simple text/plain | **173 ns** |
| `parse` multipart/alternative (2 parts) | **831 ns** |
| `find_by_content_type("text/calendar")` (full parse + walk) | **1.39 µs** |

Compared to `mail-parser` 0.11 on the same inputs:

| Path | mailrs-mime | mail-parser | ratio |
|---|---:|---:|---:|
| simple body_text | 207 ns | 194 ns | ~1.1× |
| invite-shape, first part lookup | 1.38 µs | 630 ns | ~2.2× (mail-parser faster) |

The simple case is close to a wash; the multipart walk path is
where mail-parser's optimized internals still win. Future polish
rounds may close that gap. v1.0 ships at shape parity — the data
extracted is the same; perf is acceptable for inbound where parse
cost is dwarfed by DNS / disk / DB calls.

Reproduce: `cargo bench -p mailrs-mime --bench mime`.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-mime`) |
| **test** | line cov: 96.5% (`cargo llvm-cov -p mailrs-mime --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 3 gate(s) `perf_gate.rs` |
| **size** | release rlib: 142 KB |
| **fuzz** | ✅ 1 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons (from PERFORMANCE.md)

- `mailrs-mime` vs `mail-parser` (MIME body parse)

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
