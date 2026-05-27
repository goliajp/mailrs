# Changelog

## 0.1.0 (unreleased)

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
