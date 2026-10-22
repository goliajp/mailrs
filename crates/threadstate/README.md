# mailrs-threadstate

[![Crates.io](https://img.shields.io/crates/v/mailrs-threadstate?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-threadstate)
[![docs.rs](https://img.shields.io/docsrs/mailrs-threadstate?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-threadstate)
[![License](https://img.shields.io/crates/l/mailrs-threadstate?style=flat-square)](#license)

An append-only log of per-thread decisions, stored beside the mail.
Maildir flags and keyword bits hold one bit each, which is enough for
"read" and "archived" but not for values such as:

- `snoozed_until`, a timestamp;
- `category`, a classifier's verdict at a point in time;
- `importance_level` / `importance_score`;
- `requires_action`.

Keeping them next to the Maildir means a rebuilt index can read them
back instead of losing them.

## Format

One NDJSON record per change. A record states only the fields it
changes, and a correction is a new record, never an edit:

```text
{"tid":"t-0@x","at":1786650987,"snoozed_until":1786700000}
{"tid":"t-0@x","at":1786651020,"category":"notification"}
```

These two lines leave the thread snoozed and categorised. File order
decides which value wins, not `at`: appends are ordered by construction
and wall clocks are not. `at` is recorded for people reading the log and
is never compared.

## Example

```rust
use mailrs_threadstate::Record;

let dir = std::path::Path::new("/var/mail/alice");
let mut record = Record::new("t-0@x", 1786650987);
record.snoozed_until = Some(1786700000);
mailrs_threadstate::append(dir, &record)?;

let state = mailrs_threadstate::read(dir)?;
assert_eq!(state.get("t-0@x").and_then(|r| r.snoozed_until), Some(1786700000));
# Ok::<(), std::io::Error>(())
```

## API

- `Record::new(tid, at)` plus its optional fields.
- `ThreadState::parse`, `ThreadState::get`, `ThreadState::iter`: the
  resolved state per thread.
- `ThreadState::compacted`: one record per thread with the resolved
  fields, for rewriting the file.
- `path`, `read`, `append`, `append_many`, `rewrite`: the file beside a
  mailbox directory. A crash during compaction loses nothing, because the
  uncompacted file is still valid.

## License

Apache-2.0 OR MIT.
