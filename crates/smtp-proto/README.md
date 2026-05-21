# mailrs-smtp-proto

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-proto?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-smtp-proto)
[![docs.rs](https://img.shields.io/docsrs/mailrs-smtp-proto?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-smtp-proto)
[![License](https://img.shields.io/crates/l/mailrs-smtp-proto?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-smtp-proto?style=flat-square)](https://crates.io/crates/mailrs-smtp-proto)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

SMTP protocol parser, formatter, and session state machine for Rust — zero I/O, async-runtime-agnostic.

Implements [RFC 5321] (SMTP) plus the common extensions used by every real-world mail server: STARTTLS ([RFC 3207]), AUTH PLAIN / LOGIN ([RFC 4954], [RFC 4616]), SMTPUTF8 ([RFC 6531]), enhanced status codes ([RFC 3463]), and the SIZE / 8BITMIME / PIPELINING extensions.

## Highlights

- **Zero I/O** — pure parsing and state-machine logic. No TCP, no TLS, no async runtime. The caller wires it.
- **Zero-copy parsing** — `parse_command()` returns a `Command<'_>` borrowed from the input slice.
- **Complete state machine** — `Session::handle_command()` maps commands to `Event` decisions: reply, open DATA, upgrade TLS, authenticate, shut down.
- **Named response constructors** — `Response::mail_ok()` / `Response::dnsbl_reject(...)` / `Response::greylisted()` etc. cover RFC 5321 and the anti-spam responses you actually need.
- **Battle-tested** — extracted from [mailrs], a production Rust mail server. 232 tests, no `unsafe`, one external dependency ([base64]).

## Quick start

```rust
use mailrs_smtp_proto::{parse_command, Command, Session, SessionConfig, Event};

let mut session = Session::new("smtp.example.com", SessionConfig::default());

let cmd = parse_command("EHLO client.example.org").unwrap();
assert!(matches!(cmd, Command::Ehlo("client.example.org")));

match session.handle_command(&cmd) {
    Event::Reply(resp) => {
        // write resp.format() to the wire, then read the next command
    }
    Event::NeedData { reverse_path, forward_paths } => {
        // MAIL FROM + RCPT TO + DATA all accepted; read the message body
    }
    Event::StartTls(_) => {
        // upgrade the connection, then call session.reset_after_tls()
    }
    Event::Shutdown(_) => {
        // write the response and close
    }
    Event::NeedAuth { username, password } => {
        // verify credentials externally, then call session.set_authenticated()
    }
    Event::AuthChallenge { response, step } => {
        // write the challenge, read the next line, call session.handle_auth_response()
    }
}
```

See [`examples/parse_and_drive.rs`](examples/parse_and_drive.rs) for an end-to-end walk-through of a fake EHLO / MAIL FROM / RCPT TO / DATA flow.

## What this crate does NOT do

- No I/O. No TCP, no TLS, no async runtime. The caller wires it.
- No message storage, no DKIM/SPF/DMARC.
- No outbound SMTP client. See `mailrs-smtp-client` (coming separately).

## Module overview

| Module | What it does |
|--------|--------------|
| `command` | Typed `Command<'a>` enum and its payload types (`ReversePath`, `ForwardPath`, `Param`, `AuthMechanism`). |
| `parse` | `parse_command(&str) -> Command<'_>`. Handles all RFC 5321 verbs plus AUTH and STARTTLS. |
| `response` | `Response` with named constructors for every common reply, plus `format_ehlo_response` for multi-line EHLO. |
| `session` | `Session` state machine that maps `Command` → `Event`. Tracks transaction state across EHLO / MAIL FROM / RCPT TO / DATA / RSET / STARTTLS / AUTH. |
| `auth` | SASL helpers: `decode_plain` (AUTH PLAIN) and `decode_login_response` (AUTH LOGIN). |
| `data` | `unstuff_line` / `unstuff_data` handle the dot-stuffing convention in the DATA stage. |
| `address` | Minimal `is_valid` / `split_address` helpers. |

## Why a separate crate?

`mailrs-smtp-proto` is intentionally just the protocol layer. If you're building a mail server (in any flavor — receiver, MTA, milter, MX-test tool), you almost always want to own the I/O and the authentication policy yourself; what's painful is the wire-format parsing, the state machine corners, and the long tail of response codes. That's what this crate is for.

It's also the foundation of the [mailrs] mail server, which uses it on the inbound listener side. Publishing it independently means everyone gets the same battle-tested core.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[RFC 5321]: https://datatracker.ietf.org/doc/html/rfc5321
[RFC 3207]: https://datatracker.ietf.org/doc/html/rfc3207
[RFC 4954]: https://datatracker.ietf.org/doc/html/rfc4954
[RFC 4616]: https://datatracker.ietf.org/doc/html/rfc4616
[RFC 6531]: https://datatracker.ietf.org/doc/html/rfc6531
[RFC 3463]: https://datatracker.ietf.org/doc/html/rfc3463
[mailrs]: https://github.com/goliajp/mailrs
[base64]: https://crates.io/crates/base64
