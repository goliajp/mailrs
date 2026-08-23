# mailrs-jmap-client

Reading a JMAP mailbox.

`mailrs-jmap` is the server half — a dispatcher over a `MailStore`.
This is the other direction, and it is a smaller job than IMAP was:
one HTTP endpoint, a batch of method calls, and no session state to
keep in step.

```rust
use mailrs_jmap_client::{parse_session, blob_url};

let session = parse_session(r#"{
  "apiUrl": "https://api.example.com/jmap/api/",
  "downloadUrl": "https://api.example.com/jmap/download/{accountId}/{blobId}/{name}",
  "primaryAccounts": { "urn:ietf:params:jmap:mail": "u1" },
  "accounts": {}
}"#).expect("a mail account");

assert_eq!(session.account_id, "u1");
assert_eq!(
    blob_url(&session, "Gabc", "message.eml"),
    "https://api.example.com/jmap/download/u1/Gabc/message.eml"
);
```

## Two things that decide the design

**The session object is the entry point, not the URL somebody typed.**
`GET /.well-known/jmap` answers with the API URL, the download template
and the account ids. An account built on the typed URL works until the
provider moves its endpoint — which Fastmail has done.

**`cannotCalculateChanges` means start over.** It is a state, not an
error to log: the server can no longer say what changed since the state
being asked from. A client that keeps asking anyway never sees another
message, and nothing about that looks like a failure from outside. It
is the same shape as trusting a stale `UIDVALIDITY`, and it gets a
named variant for the same reason.

## What this does not do

No push (EventSource and WebSocket are a second transport with their
own failure modes, and a poll that works beats a push that silently
stops), and no `Email/set` — writing read state back is a second
direction with its own conflict rules, and this is a reader.

## No consumer in this repository

mailrs is a mail **server**; reading somebody else's mailbox is a mail
**client** job. It was briefly wired into the server — accounts synced
into mailrs's own store — and that was the wrong place: another
server's mailbox forced through this one's thread model produced a
folder tree with nowhere to go, a Sent folder filed as received mail,
and read state living in two places at once. Removed 2026-08-23.

The crate stays because it is a stone: no business types, no mailrs
types, and useful to anything that needs to read a mailbox. Its
consumers are elsewhere.
