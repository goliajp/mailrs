# mailrs-imap-client

Reading somebody else's IMAP mailbox.

`mailrs-imap-proto` is the server's half — it *formats* `* CAPABILITY`
and *parses* the commands a client sends. This is the other half: it
parses what a server says back, and it decides what to ask for next.

The I/O is the caller's. What is here is the part that is hard to get
right and easy to test: response parsing, and the UID bookkeeping that
decides which messages are new.

```rust
use mailrs_imap_client::{FolderState, Untagged, plan_fetch, parse_line};

// What the server said when the folder was opened.
let mut state = FolderState::default();
for line in ["* 231 EXISTS", "* OK [UIDVALIDITY 1234567890] .", "* OK [UIDNEXT 4392] ."] {
    if let Some(u) = parse_line(line) { state.apply(&u); }
}

// What to ask for, given what we saw last time.
let plan = plan_fetch(&state, Some(4300));
assert_eq!(plan.map(|p| p.range), Some("4300:*".into()));
```

## The bookkeeping is the whole feature

A uid means nothing on its own. It means something *within a
uidvalidity*, and when a server changes that number every uid a client
remembers is meaningless — RFC 3501 §2.3.1.1 says so, and the classic
bug is trusting the old numbers anyway and silently missing every
message since. `FolderState` carries the validity beside the numbers so
the two cannot be separated, and `plan_fetch` returns a **full resync**
rather than a range when it changed.
