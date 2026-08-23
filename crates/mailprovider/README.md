# mailrs-mailprovider

Where a provider's servers are, and how it wants to be asked.

Adding somebody's Gmail should be three taps, and adding the mail
server their university runs should be one field and a guess. This
holds the part that is the same — a table of settings, and a ranked
list of ways to find settings that are not in the table — and keeps the
differences as data rather than as branches.

```rust
use mailrs_mailprovider::{preset_for, AuthKind};

let gmail = preset_for("someone@gmail.com").expect("known");
assert_eq!(gmail.imap.host, "imap.gmail.com");
assert_eq!(gmail.auth, AuthKind::OAuth2);

// Not in the table: the caller falls back to autodiscovery.
assert!(preset_for("someone@a-university.example").is_none());
```

Zero I/O. Autodiscovery needs DNS and HTTP, so this crate produces the
**queries to make**, in the order to make them, and reads the answers;
performing them belongs to the caller.

## What the table has to carry beyond hosts and ports

Two things decide whether a set-up screen is usable, and neither is a
hostname:

- **What the person must type.** Gmail and Outlook will not take a
  password at all; QQ and 163 want an authorisation code generated in
  their web UI, which is not the login password and is refused with a
  message that does not say so. Each preset carries which of these it
  is, and where to go and get it.
- **Whether the provider hides folders.** Gmail's `[Gmail]/All Mail`
  duplicates every message; syncing it as a folder downloads the
  mailbox twice.

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
