# mailrs-maildir

[![Crates.io](https://img.shields.io/crates/v/mailrs-maildir?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-maildir)
[![docs.rs](https://img.shields.io/docsrs/mailrs-maildir?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-maildir)
[![License](https://img.shields.io/crates/l/mailrs-maildir?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-maildir?style=flat-square)](https://crates.io/crates/mailrs-maildir)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

Maildir filesystem-format primitives for Rust — atomic delivery, directory scans, flag parsing. No protocol layer.

Implements the [Maildir] convention invented by Daniel J. Bernstein and used by qmail, Dovecot, Courier, mutt, neomutt, postfix, and most other Unix MUAs. Messages live as one file per message under `<root>/{tmp,new,cur}/`, with the filename encoding a globally unique ID plus an optional flag suffix.

## Highlights

- **Atomic delivery** — `deliver()` writes to `tmp/`, fsyncs, then renames into `new/`. This is the canonical Maildir reliability technique: no partial messages ever appear in `new/`.
- **Directory scans** — `scan_new()` / `scan_cur()` list each stage with parsed flags.
- **Filename grammar** — `parse_flags` / `serialize_flags` / `add_flag` handle the `":2,FLAGS"` suffix convention.
- **Crash-safe janitor** — `cleanup_tmp(max_age)` removes stale partial deliveries from crashed processes.
- **Battle-tested** — extracted from [mailrs], a production Rust mail server. 71 tests, no `unsafe`, one external dependency ([hostname]).

## Quick start

```rust
use mailrs_maildir::{Maildir, Flag, serialize_flags};

let md = Maildir::create("/var/mail/alice/INBOX")?;

// deliver: tmp/ → fsync → rename to new/
let id = md.deliver(b"From: a@example.com\r\nSubject: hi\r\n\r\nhello\r\n")?;

// scan
for entry in md.scan_new()? {
    println!("{} flags={:?}", entry.id, entry.flags);
}

// transition new/ → cur/ with a Seen flag (caller does the rename)
let _suffix = serialize_flags(&[Flag::Seen]);  // ":2,S"
# Ok::<(), std::io::Error>(())
```

See [`examples/deliver_and_scan.rs`](examples/deliver_and_scan.rs) for a runnable walk-through.

## What this crate does NOT do

- **No IMAP / POP3 protocol.** See `mailrs-imap-proto`.
- **No mailbox database / UID index.** The `cur/`-vs-`new/` split is the only persisted state. Anything richer (sequence numbers, threads, full-text search) lives one layer up.
- **No locking.** Maildir is designed to be lock-free: atomic rename for delivery and stage transitions.

## Maildir at a glance

```
<root>/
├── tmp/    # in-flight deliveries (file being written)
├── new/    # delivered, not yet seen by any client
└── cur/    # seen by at least one client; flags in filename suffix
```

Filenames look like `1684500000.M123456P9999Q0.hostname:2,S` — timestamp + uniqueness components + `:2,FLAGS` suffix. This crate handles the parsing and the atomic transitions; what you do with the messages is your business.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-maildir`) |
| **test** | line cov: 98.9% (`cargo llvm-cov -p mailrs-maildir --summary-only`) |
| **bench** | ✅ 2 file(s) criterion + ✅ 3 gate(s) `perf_gate.rs` |
| **size** | release rlib: 126 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[Maildir]: https://cr.yp.to/proto/maildir.html
[mailrs]: https://github.com/goliajp/mailrs
[hostname]: https://crates.io/crates/hostname
