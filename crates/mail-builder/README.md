# mailrs-mail-builder

RFC 5322 / 2046 / 2047 / 2231 outbound mail builder — the inverse
of the `mailrs-rfc5322` + `mailrs-rfc2047` + `mailrs-mime` parse
stones.

## Why

Outbound message construction in mail servers tends to grow ad-hoc
string formatting that drifts toward MIME non-compliance over time
(lone LF, mis-folded headers, bad boundaries, missing
`Content-Transfer-Encoding`). When a message looks merely "weird"
rather than "broken", receiving MTAs silently lower reputation
rather than reject — the failure mode is "we got banned without
ever seeing a 5xx". A canonical builder closes that whole class of
bug at the source.

## Scope (0.1)

- Plain-text single-part messages.
- `multipart/alternative` (text + html).
- `multipart/mixed` (body + attachments).
- Encoded-word (RFC 2047) for non-ASCII header values.
- Soft-fold (RFC 5322 §2.2.3) at 78 chars.
- CTE auto-selection: `7bit` / `quoted-printable` / `base64`.
- Boundary collision-scan (regenerate if body contains it).

## Example

```rust
use mailrs_mail_builder::{Attachment, MessageBuilder};

let msg = MessageBuilder::new()
    .from("Alice <alice@example.com>")
    .to("bob@example.com")
    .subject("こんにちは")
    .text_body("Plain text version.")
    .html_body("<p>HTML version.</p>")
    .attachment(Attachment::new("doc.pdf", "application/pdf", b"...".to_vec()))
    .build();

// msg is a Vec<u8> of canonically-compliant RFC 5322 bytes.
```

## Out of scope

- DKIM signing — use `mailrs-dkim`.
- DSN formatting — use `mailrs-outbound-queue::dsn` (will be
  migrated onto this builder in a later release).
- Calendar invites — use `mailrs-ical`.
- S/MIME, OpenPGP/MIME — out of project scope.

## Status

0.1 is the initial MVP — the API is intentionally narrow to match
mailrs's three internal use cases (DSN, DMARC report, future
TLS-RPT). Wider API surface lands in 1.0 after the deliverability
hardening pass.
