# mailrs-pop3-client

Reading a mailbox that only offers POP3.

POP3 is small, and the parsing is not the interesting part. The
interesting part is **identity**: the message numbers a session hands
out renumber the moment anything is deleted, so a client that remembers
those downloads the mailbox again on every sync. The only durable
identity POP3 offers is `UIDL`.

```rust
use mailrs_pop3_client::{parse_uidl, not_yet_held};

let on_server = parse_uidl(&["1 whqtswO00WBw418f9t5", "2 QhdPYR:00WBw1Ph7x7"]);
let held = ["whqtswO00WBw418f9t5".to_string()];
let want = not_yet_held(&on_server, &held);
assert_eq!(want.len(), 1);
assert_eq!(want[0].number, 2);
```

`UIDL` is optional in RFC 1939. A server without it **cannot be
deduplicated at all**, so that is a named error rather than a general
failure — the person needs to be told when they connect the account,
not have their mailbox re-downloaded every hour for as long as it
exists.

The socket half is behind the `net` feature, so this stays testable
without a TLS stack.

## The one that corrupts mail

Multi-line responses are dot-stuffed (RFC 1939 §3): a body line
beginning with `.` arrives doubled. Undo it or every message
containing such a line is corrupted — quoted mail and `..` in code
both produce them.
