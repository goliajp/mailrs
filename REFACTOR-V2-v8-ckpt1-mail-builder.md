# v8 ckpt 1 — mailrs-mail-builder 0.1 MVP

New workspace member `crates/mail-builder/` shipping the 0.1
canonically-compliant outbound message builder. The inverse of the
mailrs parse stones (`mailrs-rfc5322` + `mailrs-rfc2047` +
`mailrs-mime`) — same MIME compliance invariants applied to the
write side.

**Not yet wired into production paths.** The existing
`outbound-queue::dsn::format_dsn` and `dmarc::format_report_email`
continue to ship the bytes they ship today. The MVP is in-tree
behind its own `mailrs-mail-builder` crate; the prod swap is ckpt 3
after the deliverability hardening pass (ckpt 2).

## What got built

### Crate scaffold

- `crates/mail-builder/` workspace member
- `Cargo.toml` — depends on `mailrs-rfc2047` (encoded-word) + `base64`
  + `chrono` (for default `Date:`); no other prod deps
- `README.md` + `CHANGELOG.md` + lib.rs façade
- 3 modules: `encode` (CTE + qp / base64 / fold helpers),
  `multipart` (boundary + envelope), `builder` (`MessageBuilder`,
  `Attachment`)
- All three modules under the 500-LOC hard limit

### Public API (0.1)

| Item | Purpose |
|---|---|
| `MessageBuilder` | chain-style builder; `.from() .to() .cc() .bcc() .subject() .text_body() .html_body() .attachment() .header() .date() .message_id() .build()` |
| `MessageBuilder::build() -> Vec<u8>` | canonical RFC 5322 bytes |
| `impl Display for MessageBuilder` | UTF-8 view of `build()` |
| `Attachment::new(filename, ct, data)` | one attachment per multipart/mixed part |
| `ContentTransferEncoding` | enum: `SevenBit` / `EightBit` / `QuotedPrintable` / `Base64` |
| `choose_cte(body) -> ContentTransferEncoding` | the auto-select heuristic exposed for testing / external use |
| `generate_boundary()` | mailrs-prefixed boundary string |
| `multipart_envelope(parts) -> (boundary, bytes)` | low-level envelope assembly with collision-scan |

### Heuristics

1. **CTE auto-select**:
   - embedded NUL or > 15 % ASCII-control density → `Base64`
   - any byte > 0x7F or any line > 78 chars → `QuotedPrintable`
   - otherwise → `7bit`
   - Important fix during impl: high-bit bytes (UTF-8 continuation)
     are NOT counted toward the "non-printable" ratio — UTF-8 text
     would otherwise be mis-routed to base64.

2. **Encoded-word**: `maybe_encode_word(value)` passes ASCII through
   unchanged, calls `mailrs_rfc2047::encode` (which picks B vs Q
   internally) for non-ASCII.

3. **Header folding**: 78-char soft wrap at whitespace boundaries.
   Tokens longer than the soft limit are emitted on their own
   continuation line without breaking the token.

4. **Boundary generation**: `mailrs_<pid>_<counter>_<rng>` with
   collision-scan against the actual body bytes; on collision a
   fresh boundary is drawn (capped at 8 attempts — practically
   unreachable).

5. **Quoted-printable**: RFC 2045 §6.7 escaping; soft line break
   (`=\r\n`) before column 76; trailing whitespace on a line is
   always escaped.

6. **Base64**: RFC 2045 §6.8 with `\r\n` every 76 chars.

### Tests (38 total)

- 35 inline unit tests across the three modules
- 3 integration tests (`tests/use_cases.rs`):
  - `dsn_shaped_message_builds_and_parses` — proves the builder can
    produce a DSN-shaped message (multipart/mixed with a
    `message/delivery-status` attachment part). Byte-equivalent
    replacement of `format_dsn` is ckpt 3 work.
  - `dmarc_report_message_builds_and_parses` — proves the builder
    can produce the DMARC aggregate report email shape (`text` +
    gzipped XML attachment).
  - `builder_roundtrips_through_mime_parse` — uses `mailrs-mime`
    to parse the builder's output and verify multipart structure.

## Coverage

`cargo llvm-cov -p mailrs-mail-builder --summary-only`:

| Module | Lines | Functions |
|---|---:|---:|
| `builder.rs` | 93.56 % | 97.67 % |
| `encode.rs` | 97.54 % | 100.00 % |
| `multipart.rs` | 94.34 % | 100.00 % |
| **TOTAL** | **94.84 %** | **98.72 %** |

Comfortably above the v8 ckpt 1 → 2 trigger (`mailrs-mail-builder
0.1 跑通 DSN + DMARC report 2 个用例 · 内部 unit test ≥ 90 %`).

## What's deliberately NOT in 0.1

- `multipart/report` (RFC 3464) — needs a typed variant; 1.0 work.
- DKIM signing — that's the `mailrs-dkim` stone's job; builder
  emits the message, signer adds the `DKIM-Signature:` header.
- Calendar invites — `mailrs-ical` already covers `text/calendar`
  body construction.
- Streaming output — `build()` produces a `Vec<u8>` since real
  outbound messages are bounded (< few MB) and the simplicity is
  worth more than the streaming complexity for 0.1.
- S/MIME, OpenPGP/MIME — out of project scope.

## Next steps

- **ckpt 2** — deliverability hardening: RFC test corpus + 1000
  proptest samples differential parse vs `mail-parser` +
  cross-MTA interop (Postfix + Mailpit container).
- **ckpt 3** — swap `format_dsn` + `format_report_email` onto
  `mailrs-mail-builder`, publish 1.0 to crates.io, deploy + 48-72h
  prod observation.

No new prod dependencies. No production paths touched. Safe to
release as a workspace-internal stone.
