# mailrs-imap-proto

[![Crates.io](https://img.shields.io/crates/v/mailrs-imap-proto?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-imap-proto)
[![docs.rs](https://img.shields.io/docsrs/mailrs-imap-proto?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-imap-proto)
[![License](https://img.shields.io/crates/l/mailrs-imap-proto?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-imap-proto?style=flat-square)](https://crates.io/crates/mailrs-imap-proto)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

IMAP4rev1 protocol parser, response formatter, and sequence-set helpers for Rust — zero I/O, async-runtime-agnostic.

Implements the wire-format pieces of [RFC 3501] (IMAP4rev1): tagged command parsing, sequence-set arithmetic, SEARCH key parsing, and the common untagged + tagged response formatters. Connection state, mailbox storage, and the IDLE / AUTHENTICATE message pump are the caller's job.

## Highlights

- **Zero I/O** — pure parsing + formatting. No TCP, no TLS, no async runtime.
- **Typed commands** — `parse_command()` returns a `TaggedCommand { tag, command: ImapCommand }`. The `ImapCommand` enum covers LOGIN / SELECT / FETCH / STORE / SEARCH / IDLE / APPEND / UID-prefixed variants / etc.
- **Sequence sets** — `parse_sequence_set("1,3:5,7:*")` → typed `SequenceSet`; `sequence_set_to_uids(&set, max)` → sorted deduped UID list. Handles `*`, ranges, lists, out-of-range clamping.
- **Search keys** — `parse_search_criteria()` returns typed `Vec<SearchKey>` (FROM / TO / SUBJECT / TEXT / BODY / SEEN / UNSEEN / SINCE / BEFORE / UID / ...).
- **Response formatters** — `format_ok` / `format_no` / `format_bad` (tagged); `format_capability` / `format_list` / `format_fetch` / `format_flags` / `format_exists` / `format_recent` / `format_bye` / `format_quota` / `format_quotaroot` (untagged).
- **Battle-tested** — extracted from [mailrs], a production Rust mail server. 225 tests, no `unsafe`, zero external dependencies.

## Quick start

```rust
use mailrs_imap_proto::{
    parse_command, parse_sequence_set, sequence_set_to_uids,
    format_capability, format_fetch, format_ok, ImapCommand,
};

// parse a tagged command line
let parsed = parse_command("a001 CAPABILITY").unwrap();
assert_eq!(parsed.tag, "a001");
assert_eq!(parsed.command, ImapCommand::Capability);

// expand a sequence set against a mailbox of 8 messages
let set = parse_sequence_set("1,3:5,7:*").unwrap();
let uids = sequence_set_to_uids(&set, 8);
assert_eq!(uids, vec![1, 3, 4, 5, 7, 8]);

// format a few responses
let _ = format_capability(&["IMAP4rev1", "IDLE", "AUTH=PLAIN"]);
let items = vec![
    ("FLAGS".to_string(), "(\\Seen)".to_string()),
    ("UID".to_string(), "42".to_string()),
];
let _ = format_fetch(1, &items);
let _ = format_ok("a001", "CAPABILITY completed");
```

See [`examples/parse_and_format.rs`](examples/parse_and_format.rs) for a longer walk-through.

## What this crate does NOT do

- No I/O. No TCP, no TLS, no async runtime, no connection management.
- No mailbox storage or message indexing.
- **No session state machine.** Unlike SMTP, IMAP's per-connection state (selected mailbox, capability negotiation, pending IDLE / authenticate continuations, command literal handling) is owned by the caller. This crate gives you typed commands in and formatted lines out — you keep the state.

## Module overview

| Module | What it does |
|--------|--------------|
| `command` | `parse_command(&str) -> TaggedCommand`. The `ImapCommand` enum + `SearchKey` enum + `ParseError`. |
| `sequence` | `parse_sequence_set` / `sequence_set_to_uids`. Handles `*`, ranges, lists, clamping. |
| `response` | `format_*` functions for both tagged (OK/NO/BAD) and untagged (CAPABILITY/LIST/FETCH/...) responses. |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[RFC 3501]: https://datatracker.ietf.org/doc/html/rfc3501
[mailrs]: https://github.com/goliajp/mailrs
