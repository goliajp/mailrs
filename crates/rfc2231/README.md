# mailrs-rfc2231

[![Crates.io](https://img.shields.io/crates/v/mailrs-rfc2231?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-rfc2231)
[![docs.rs](https://img.shields.io/docsrs/mailrs-rfc2231?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-rfc2231)
[![License](https://img.shields.io/crates/l/mailrs-rfc2231?style=flat-square)](#license)

RFC 2231 MIME parameter encoder + decoder. Handles non-ASCII parameter
values in `Content-Type` / `Content-Disposition`:

```text
Content-Disposition: attachment; filename*=UTF-8''%E6%97%A5%E6%9C%AC.pdf
                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                                          this — encode + decode
```

Companion to [`mailrs-rfc2047`](https://crates.io/crates/mailrs-rfc2047)
(which handles `=?charset?(B|Q)?…?=` in header values like Subject /
From). Together they cover the full MIME header encoding surface a
typical SMTP server needs.

## Quickstart

```rust
use mailrs_rfc2231::{encode_param, decode_param_value};

// Encode (outbound): UTF-8 source → wire format
let header_line = format!(
    "Content-Disposition: attachment; {}",
    encode_param("filename", "日本.pdf"),
);
// → "Content-Disposition: attachment; filename*=UTF-8''%E6%97%A5%E6%9C%AC.pdf"

// Decode (inbound): wire format → UTF-8
let v = decode_param_value("UTF-8''%E6%97%A5%E6%9C%AC.pdf");
assert_eq!(v.as_deref(), Some("日本.pdf"));

// Legacy quoted form is also accepted on decode.
assert_eq!(decode_param_value("\"test.pdf\"").as_deref(), Some("test.pdf"));
```

## What this crate does

- **encode_param(name, value)** — emits `name="value"` for pure ASCII,
  `name*=UTF-8''<percent-encoded>` for non-ASCII. UPPERCASE hex
  (RFC-canonical).
- **decode_param_value(s)** — accepts the three real-world shapes:
  - legacy quoted: `"some value"` (strips quotes, unescapes backslashes)
  - legacy unquoted bareword: `attachment`
  - RFC 2231 extended: `charset'lang'percent-encoded` (charset
    resolved via `encoding_rs`, language tag discarded)
- Lenient percent-decode: `%X<non-hex>` and lone `%` are passed
  through, not rejected.

## What this crate does not

- **Continuation parameters** (`filename*0=…; filename*1=…` per
  RFC 2231 §3). Rare in practice; can be added in 1.x without breaking
  compat if needed.
- **Header-line parsing** — you give it just the parameter value, not
  the full `Content-Type: …; foo=…; bar=…` line. Use `mailrs-rfc5322`
  (or your favorite header parser) to split the line first.
- **MIME-tree parsing** — this only does parameter values, not the
  whole MIME structure. Use `mail-parser` or focused MIME crate.

## When to reach for this

| Use case | This crate or alternative? |
|---|---|
| Build a `Content-Disposition` line with an i18n filename | **mailrs-rfc2231** |
| Parse `filename*=…` from an inbound attachment header | **mailrs-rfc2231** |
| Full MIME-tree parsing with body decode | `mail-parser` |
| Encode/decode encoded-words in Subject/From | `mailrs-rfc2047` |

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `encode_param` (ASCII, legacy quoted) | **30 ns** |
| `encode_param` (Japanese, extended form) | **128 ns** |
| `encode_param` (60-char Japanese filename) | **448 ns** |
| `decode_param_value` (legacy quoted) | **9 ns** |
| `decode_param_value` (legacy bareword) | **6 ns** |
| `decode_param_value` (UTF-8 extended) | **100 ns** |
| `decode_param_value` (ISO-8859-1 extended) | **133 ns** |

Reproduce: `cargo bench -p mailrs-rfc2231 --bench params`. Workspace
[PERFORMANCE.md](../../PERFORMANCE.md) carries the same table.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-rfc2231`) |
| **test** | line cov: 99.5% (`cargo llvm-cov -p mailrs-rfc2231 --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 4 gate(s) `perf_gate.rs` |
| **size** | release rlib: 33 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
