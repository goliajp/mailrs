# mailrs-ical

[![Crates.io](https://img.shields.io/crates/v/mailrs-ical?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-ical)
[![docs.rs](https://img.shields.io/docsrs/mailrs-ical?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-ical)
[![License](https://img.shields.io/crates/l/mailrs-ical?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-ical?style=flat-square)](https://crates.io/crates/mailrs-ical)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

RFC 5545 (iCalendar) + RFC 5546 (iTIP) parser, serializer, and typed semantics for Rust — hand-rolled, zero I/O, with VTIMEZONE + RRULE support.

Extracted from [mailrs] so any Rust project that needs to ingest `text/calendar` invites (or generate `REPLY` payloads) can lean on the same battle-tested core: byte-by-byte parsing with no parser-combinator dependency, typed `Method` / `Attendee` / `Organizer` / `CalDateTime`, and inline VTIMEZONE handling with chrono-tz IANA fallback.

## Highlights

- **Zero I/O** — pure parsing and formatting. No file system, no network, no async runtime. The caller wires everything.
- **Typed semantics** — [`parse_invite`] returns a fully-typed [`ParsedInvite`] with `METHOD` / `UID` / `SEQUENCE` / `DTSTAMP` / `DTSTART` / `DTEND` / `ATTENDEE` / `ORGANIZER` / `RRULE` / `EXDATE` / `RDATE` / `RECURRENCE-ID` / `STATUS` / `SUMMARY` / `LOCATION` / `DESCRIPTION` / `VTIMEZONE`.
- **VTIMEZONE smart fallback** — accepts inline VTIMEZONE blocks per RFC 5545; falls back to chrono-tz IANA names when the TZID is a known location.
- **iTIP awareness** — [`Method`] enum covers `REQUEST` / `REPLY` / `CANCEL` / `UPDATE` / `COUNTER` / `REFRESH` / `ADD` / `PUBLISH` / `DECLINECOUNTER` per RFC 5546.
- **Serializer** — [`serialize`] turns a [`ParsedInvite`] back into RFC 5545 text, suitable for iTIP `REPLY` bodies.
- **Battle-tested** — extracted from a production Rust mail server; verified against a corpus of real `.eml` fixtures from Outlook / Nextcloud / Google / Apple Calendar / Thunderbird.

## Quick start

```rust
use mailrs_ical::{parse_invite, Method};

let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\n\
            PRODID:-//Example//Cal//EN\r\nBEGIN:VEVENT\r\n\
            UID:abc\r\nDTSTAMP:20260101T000000Z\r\n\
            DTSTART:20260102T100000Z\r\nSUMMARY:Lunch\r\n\
            END:VEVENT\r\nEND:VCALENDAR\r\n";

let invite = parse_invite(ics).unwrap();
assert_eq!(invite.method, Method::Request);
assert_eq!(invite.uid, "abc");
assert_eq!(invite.summary, "Lunch");
```

See [`examples/parse_invite.rs`](examples/parse_invite.rs) for a walk-through that parses an invite, prints the typed view, and round-trips it through `serialize`.

## What this crate does NOT do

- No MIME parsing — extract the `text/calendar` part upstream (e.g. with [`mail-parser`]).
- No SMTP — see [`mailrs-smtp-proto`] / [`mailrs-smtp-client`].
- No calendar storage, no CalDAV server. This is the wire-format layer only.
- No RRULE _expansion_ to concrete instances — the parser captures the raw RRULE string; consumers expand with the [`rrule`] crate as needed.

## Module overview

| Module       | What it does |
|--------------|--------------|
| `parse`      | RFC 5545 §3.1 text → raw AST (line folding, property tree, BEGIN/END nesting). |
| `semantics`  | AST → typed [`ParsedInvite`] (METHOD, ATTENDEE, ORGANIZER, SEQUENCE, RRULE, …). |
| `vtimezone`  | Inline VTIMEZONE handling with chrono-tz IANA fallback. |
| `serialize`  | [`ParsedInvite`] → RFC 5545 text (for iTIP REPLY). |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[mailrs]: https://github.com/goliajp/mailrs
[`mail-parser`]: https://crates.io/crates/mail-parser
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
[`rrule`]: https://crates.io/crates/rrule
