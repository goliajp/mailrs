# Changelog

## 1.0.0

- Promoted to 1.0 after the v8 ckpt 2 deliverability-hardening pass
  (RFC corpus + 1000-sample differential parse + Mailpit cross-MTA
  interop + strict-mode lint) and the in-tree swap of
  `mailrs-outbound-queue::dsn::format_dsn` and
  `mailrs-dmarc::format_report_email` onto the builder.
- API surface stable for the four mailrs internal use cases (DSN,
  DMARC aggregate report, TLSRPT planned for v9, plus generic
  outbound).
- New since 0.x:
  - `MessageBuilder::report_type(...)` — multipart/report support
    for RFC 6522 / 3464 / 7489.
  - `MessageBuilder::build_strict()` + `lint(raw)` invariant
    checker.
  - Attachment CTE selection: text/* and message/* parts route
    through `choose_cte` (so a `message/delivery-status` body
    inside a DSN ships as 7bit ASCII, not base64); all other
    attachments force base64.

## 0.1.0

- Initial release.
- `MessageBuilder` with chain-style setters for the standard
  RFC 5322 headers and 0-N attachments.
- Encoded-word (RFC 2047) auto-applied to non-ASCII header values
  via `mailrs-rfc2047`.
- Header folding per RFC 5322 §2.2.3 (78-char soft wrap).
- CTE auto-selection: 7bit / quoted-printable / base64 based on
  byte composition + max line length.
- `multipart/alternative` (text + html) and `multipart/mixed`
  (body + attachments).
- Multipart boundary collision-scan (regenerate if body matches).
- Output: raw `Vec<u8>` via `build()` or UTF-8 via `Display`.
