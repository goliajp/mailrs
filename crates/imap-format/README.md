# mailrs-imap-format

[![Crates.io](https://img.shields.io/crates/v/mailrs-imap-format.svg)](https://crates.io/crates/mailrs-imap-format)
[![Docs.rs](https://docs.rs/mailrs-imap-format/badge.svg)](https://docs.rs/mailrs-imap-format)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

IMAP wire-format helpers (RFC 9051 §6.4 FETCH responses, §7.5
BODYSTRUCTURE assembly, §9 ABNF for FLAGS / INTERNALDATE).

22 standalone **pure functions** — no I/O, no async. Pairs with
[`mailrs-imap-proto`](https://crates.io/crates/mailrs-imap-proto)
(command parsing + state machine) and
[`mailrs-imap-codec`](https://crates.io/crates/mailrs-imap-codec)
(line + literal framing) to form a complete RFC 9051 IMAP receive
+ response stack.

## What's in the box

| Concern | Functions |
|---|---|
| **FLAGS** | `format_imap_flags` / `parse_imap_flags` + `FLAG_*` `pub const`s |
| **INTERNALDATE** | `format_internal_date(i64)` (Unix timestamp → IMAP date) |
| **String quoting** | `escape_imap_string` / `escape_imap_str` / `quote_or_nil` |
| **Address structures** | `format_imap_address` / `format_addr_list` |
| **BODY[] section parsing** | `parse_header_fields_request` / `parse_generic_body_sections` |
| **MIME walk** | `extract_header_section` / `extract_body_section` / `extract_header_fields` / `parse_mime_headers` (→ `MimeInfo`) / `split_mime_parts` / `find_line_offset` / `trim_part_trailing_newline` / `extract_mime_part` |
| **BODYSTRUCTURE** | `build_bodystructure` (top-level, recurses into multipart) |

## Quick start

```rust
use mailrs_imap_format::{
    format_imap_flags, parse_imap_flags,
    FLAG_SEEN, FLAG_FLAGGED, FLAG_ANSWERED,
    format_internal_date, build_bodystructure,
};

// FLAGS round-trip.
let bits = FLAG_SEEN | FLAG_FLAGGED;
assert_eq!(format_imap_flags(bits), "\\Seen \\Flagged");
assert_eq!(parse_imap_flags("(\\Seen \\Flagged)"), bits);

// INTERNALDATE.
let date = format_internal_date(1_700_000_000);
assert!(date.contains("2023"));

// BODYSTRUCTURE — recurses into multipart trees.
let msg = b"Content-Type: text/plain; charset=UTF-8\r\n\r\nHello world";
let bs = build_bodystructure(msg);
assert!(bs.contains("\"text\" \"plain\""));
```

## FLAG_* bit assignments

The 6 standard IMAP system flags are exposed as `pub const u32`:

| Const | Bit | IMAP name |
|---|---|---|
| `FLAG_SEEN` | 0 | `\Seen` |
| `FLAG_ANSWERED` | 1 | `\Answered` |
| `FLAG_FLAGGED` | 2 | `\Flagged` |
| `FLAG_DELETED` | 3 | `\Deleted` |
| `FLAG_DRAFT` | 4 | `\Recent` |
| `FLAG_RECENT` | 5 | `\Recent` |

Callers that store flags in their own `u32` (e.g. a mailbox
storage layer) should match these bit positions to avoid an
intermediate mapping step. RFC 9051 doesn't mandate a bit layout
— this is the convention `mailrs-mailbox` and the wider mailrs
project use.

## What this crate does NOT do

- No IMAP **command parsing** — that's `mailrs-imap-proto`.
- No **TCP/TLS framing** — that's `mailrs-imap-codec`.
- No IMAP **state machine** (auth / select / fetch / idle) —
  that's `mailrs-imap-proto::Session`.
- No **mailbox storage** — that's `mailrs-mailbox` / `mailrs-maildir`.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ❌ 4 errors, 0 warnings (`cargo doc --no-deps -p mailrs-imap-format`) |
| **test** | line cov: 64.5% (`cargo llvm-cov -p mailrs-imap-format --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 2 gate(s) `perf_gate.rs` |
| **size** | release rlib: 243 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
