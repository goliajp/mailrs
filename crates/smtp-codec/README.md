# mailrs-smtp-codec

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-codec.svg)](https://crates.io/crates/mailrs-smtp-codec)
[![Docs.rs](https://docs.rs/mailrs-smtp-codec/badge.svg)](https://docs.rs/mailrs-smtp-codec)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Tokio `Decoder`/`Encoder` for the **RFC 5321 SMTP wire format**, with
built-in defence against the [SMTP-smuggling](https://www.postfix.org/smtp-smuggling.html)
attack class (CVE-2023-51764 and family).

Pairs with [`mailrs-smtp-proto`](https://crates.io/crates/mailrs-smtp-proto):
this crate owns the **wire I/O** (line framing, DATA-mode dot-terminator
detection, smuggle protection), `mailrs-smtp-proto` owns the
**protocol state machine** (verb parsing, session transitions).

## Why

Every Rust SMTP receiver needs to:

1. Read CRLF-terminated command lines, capped at the RFC 5321 §4.5.3.1.4
   1024-octet limit.
2. After `DATA`, switch to a different framing: raw bytes until the
   `CRLF.CRLF` dot-terminator, optionally capped at the receiver's
   advertised `SIZE`.
3. Defend against bare-LF smuggling: an attacker who can inject a
   `\n.\r\n` mid-payload can terminate the outer envelope early and
   smuggle a second message through the remainder.

`mailrs-smtp-codec` does all three in ~150 LOC. Drop into any
`tokio_util::codec::Framed`-based SMTP receiver.

## Quick start

```rust
use mailrs_smtp_codec::{SmtpCodec, SmtpInput, SmuggleProtection};
use tokio_util::codec::Decoder;
use bytes::BytesMut;

let mut codec = SmtpCodec::new()
    .with_smuggle_protection(SmuggleProtection::Strict)
    .with_max_message_size(10 * 1024 * 1024);

// Command-mode framing — CRLF-terminated lines, ≤1024 octets.
let mut buf = BytesMut::from("EHLO mail.example.org\r\n".as_bytes());
match codec.decode(&mut buf).unwrap() {
    Some(SmtpInput::Command(s)) => assert_eq!(s, "EHLO mail.example.org"),
    _ => unreachable!(),
}

// After responding 354 to DATA, switch to data mode.
codec.enter_data_mode();

let mut payload = BytesMut::from("From: a@b\r\n\r\nhello\r\n.\r\n".as_bytes());
match codec.decode(&mut payload).unwrap() {
    Some(SmtpInput::Data(bytes)) => { /* deliver */ }
    Some(SmtpInput::DataRejected) => { /* 5xx the message */ }
    _ => unreachable!(),
}
```

## Smuggle-protection policies

| Mode | Behaviour | Use when |
|---|---|---|
| `Strict` | Reject the payload if a bare-LF dot-terminator is detected | High-trust path where false positives are acceptable |
| `Permissive` (default) | Accept the payload but normalize all line endings to CRLF, destroying any smuggled envelope in transit | General-purpose receivers |
| `Off` | Pass through verbatim, RFC 5321 strict | Legacy compatibility |

`has_smuggle_sequence()` and `normalize_line_endings()` are exposed
`pub` so callers can run them independently for metrics, logging, or
custom policies without committing to one of the three modes.

## What this crate does NOT do

- No SMTP **verb parsing** — that's `mailrs-smtp-proto`.
- No SMTP **state machine** — that's `mailrs-smtp-proto::SessionState`.
- No **TLS / STARTTLS** — that's `tokio-rustls` + your session layer.
- No **MTA logic** (auth, alias resolution, delivery) — that's caller territory.

## License

Licensed under either of **Apache-2.0** ([LICENSE-APACHE](./LICENSE-APACHE))
or **MIT** ([LICENSE-MIT](./LICENSE-MIT)) at your option.
