# Architecture

mailrs is a mail server — SMTP in and out, IMAP, POP3, JMAP, ManageSieve,
CalDAV/CardDAV, a REST and MCP API, and a web client — built as a Cargo
workspace of 72 crates. Production runs four mailrs processes from one
image beside a `kevy-server`. There is no SQL database in that stack:
message metadata lives in an embedded key-value store, mail bodies are
Maildir files.

## The running system

```
                 :25 :465 :587                       :143 :993 :110 :995 :4190
                       │                                        │
 internet ──► receiver ──spool files──► fastcore ◄──── IMAP / POP3 / ManageSieve clients
                 │   "spooled" event ──►   │  ▲
                 │                         │  │ core-api (HTTP :3301)
                 │                         │  │
                 ▼                         ▼  │
            ┌──────────────── kevy-server ────┴─────────┐         webapi ◄── browser, apps,
            │ sessions, queues, side state, change feed │◄────────┘          MCP agents (:3100)
            └───────────────────────────────────────────┘
                                ▲
                       fastcore-sender ──► remote MX (DKIM-signed)
```

All four mailrs processes are the same image (`Dockerfile`) started with a
different entrypoint (`deploy/docker-compose.prod.yml`).

| process | binary | crate | what it does |
|---|---|---|---|
| receiver | `mailrs-receiver` | `crates/receiver` | Accepts SMTP on 25 (STARTTLS), 465 (implicit TLS) and 587 (submission). Runs the inbound pipeline — rate limits, PTR, DNSBL, greylisting, SPF/DKIM/DMARC/ARC, Bayes, scripted fraud rules, content scan — and refuses unknown local recipients at RCPT by asking fastcore. Accepted mail is written to the spool with its envelope; nothing else. |
| fastcore | `mailrs-fastcore` | `crates/fastcore` | Owns the mailbox. Drains the spool (alias and group resolution, Sieve, delivery into Maildir), threads and indexes every message, serves the core-api to the other processes, and runs IMAP, POP3 and ManageSieve. Also the background work: maildir self-heal, bounce filing, attachment text and OCR, snooze, push notifications, report ingestion, AOF compaction. |
| webapi | `mailrs-webapi` | `crates/webapi` | The HTTP surface: REST for the web client and the iOS/Android apps, JMAP, CalDAV/CardDAV, OIDC sign-in, the MCP server, and the `/api/events` WebSocket. Serves the web client's static files. Holds no mailbox state of its own; it asks fastcore over core-api. |
| fastcore-sender | `mailrs-fastcore-sender` | `crates/fastcore` (bin) | Takes outbound jobs off a kevy queue, DKIM-signs, delivers over MX with STARTTLS and MTA-STS, retries with backoff for up to five days, and queues a DSN back for fastcore to file when delivery fails for good. |
| kevy-server | — | `ghcr.io/goliajp/kevy` | The network key-value store the four share. |

fastcore starts `mailrs-fastcore-ocr` as a short-lived child process per
attachment that needs OCR, with its own memory and CPU limits.
`mailrs-fraud-check` (from `crates/fraud-lua`) is a command-line tool in
the same image for validating and comparing fraud rule files before they
are swapped in.

## Where state lives

| state | where | written by |
|---|---|---|
| Mail bodies | Maildir files under `MAILRS_MAILDIR`, one directory per mailbox, Maildir++ subfolders for Sent, Drafts, Junk… | fastcore (delivery), webapi (the Sent copy of what a user sends) |
| Threads, messages, per-user rows, UIDs, flags, the search index | fastcore's embedded kevy store (`MAILRS_KEVY_DATA_DIR`), AOF + snapshots | fastcore only; every other process goes through core-api |
| Sessions, outbound queue, bounce queue, aliases, Sieve scripts, contacts, calendars and address books, drafts, audit log, Bayes and triage corpora, greylist and rate-limit state, sender lists | `kevy-server` | whichever process owns the feature; most keys more than one process touches are defined in `crates/core-sidestate` |
| Inbound spool | `MAILRS_SPOOL_ROOT/incoming/` | receiver writes, fastcore drains |
| Per-mailbox files beside the mail: `dovecot-uidlist`, `dovecot-keywords`, a thread-state log | the mailbox's Maildir directory | fastcore |

The two kevy stores are not interchangeable. Mailbox data is in fastcore's
embedded store and nothing else opens it; a handler that needs it calls
core-api. The network store is for state several processes share.

## How mail moves

**Inbound.** The receiver runs the pipeline during the SMTP transaction, so
a rejection is an SMTP reply rather than a bounce. An accepted message is
written atomically to `incoming/new/` with its envelope (sender, recipients,
target folder, fraud verdict) in a header, and the receiver publishes a
`SpoolDelivered` event on kevy-server. fastcore wakes on that event (and on
a 15-second timer, for when the subscription is down), resolves each
recipient to a mailbox, an email group's members or an alias target,
applies the recipient's Sieve script, writes the file into the Maildir,
and indexes it: thread resolution, the per-user row, the text index,
importance, attachment text. Only then does it announce the message.
Everything in that sequence except the writes to the store is
`mailrs-intake`, which the Postgres-backed core runs as well.

**Realtime.** fastcore writes a `NewMessage` event under
`mailrs:events:notify:` in kevy-server. webapi reads those keys off kevy's
change feed and sends each to the WebSocket connections of the user it is
for, and no others. IMAP IDLE sessions in fastcore watch the embedded
store's own change feed.

**Outbound.** webapi validates a send, writes a job to the outbound queue in
kevy-server, writes the Sent copy into the sender's Maildir and asks
fastcore to index it. fastcore-sender delivers the job. A permanent failure
becomes a DSN on the bounce queue, which fastcore files into the sender's
inbox like any other message.

**Reading.** Lists, threads, search and flag changes are core-api calls
from webapi to fastcore. Search runs on kevy's built-in text index over
subjects, senders and previews, message bodies (first 8 KiB) and attachment
text; CJK text is indexed as character bigrams, so Japanese and Chinese
need no dictionary. webapi reads and parses message bodies from the Maildir
itself, resolving a message's `blob_ref` (`.Sent/<file>` for a subfolder)
with `mailrs_message_store::locate`.

## Crates

Libraries own one RFC or one concept each and do no I/O of their own where
they parse; the process crates own all async I/O and the wiring. Most
libraries are published on [crates.io](https://crates.io/users/golia-jp).

### Processes and their contract

| crate | role |
|---|---|
| `mailrs-receiver` | The receiver process |
| `mailrs-fastcore` | fastcore, fastcore-sender, the OCR child and maintenance tools |
| `mailrs-webapi` | The webapi process |
| `mailrs-core-api` | The core-api: request and response types, paths, client and server stubs |
| `mailrs-core-sidestate` | Network-kevy state shared by more than one process — key names and the functions that read and write them |
| `mailrs-core` | The SMTP event bus and spool envelope shared by the receiver and fastcore |
| `mailrs-intake` | The spool drain both cores run: recipient routing, groups, mock addresses, Sieve, report ingestion, what a message says about itself, and the network-kevy side effects of an arrival; each core supplies its directory and its store writes |

### Mailbox storage

| crate | role |
|---|---|
| `mailrs-mailbox-kevy` | The mailbox store on embedded kevy: thread table, per-user rows, UIDs, flags, text index |
| `mailrs-mailbox` | Store-agnostic mailbox types, flag constants and threading helpers; the Postgres store behind its `pg` feature |
| `mailrs-maildir` (`crates/storage-maildir`) | Maildir primitives: atomic delivery, scans, flags |
| `mailrs-message-store` | Delivered-mail storage trait with a Maildir implementation, and `blob_ref` resolution |
| `mailrs-delivery-executor` | Group-commit delivery on top of Maildir batch writes |
| `mailrs-alias-store`, `mailrs-alias-store-net` | The alias table trait and its kevy-server implementation |
| `mailrs-uidlist`, `mailrs-keywords`, `mailrs-threadstate` | Dovecot-compatible UID and keyword files, and a per-thread decision log, kept beside the mail so a rebuilt index loses nothing |

### Inbound pipeline and classification

| crate | role |
|---|---|
| `mailrs-inbound` | Composable receive pipeline: stage trait, early-reject executor, RFC 8601 results |
| `mailrs-shield` | DNSBL queries, greylisting, PTR/FCrDNS |
| `mailrs-rate-limit`, `mailrs-auth-guard` | Token-bucket rate limits; failed-login counting with backoff lockout |
| `mailrs-bayes`, `mailrs-triage` | Bayesian spam classification; inbox / notification / promotion triage over kevy |
| `mailrs-fraud`, `mailrs-fraud-lua`, `mailrs-textguard` | Fraud signals authentication cannot see; scripted fraud rules in sandboxed Lua; deceptive characters in display names |
| `mailrs-clamav` | ClamAV (clamd) client |
| `mailrs-intelligence` | LLM-assisted extraction and scoring behind a provider trait (off unless configured) |
| `mailrs-clean` | HTML sanitising, tracking-pixel detection, quoted-reply splitting |
| `mailrs-attachment-extract`, `mailrs-ocr` | Text from PDF attachments; OCR of images and scanned pages (Chinese, Japanese, English) in pure Rust |
| `mailrs-mock` | Disposable test mailboxes: address grammar and the parsed view of a captured message |

### Protocols and formats

| crate | role |
|---|---|
| `mailrs-smtp-proto`, `mailrs-smtp-codec` | SMTP parser, formatter and session state machine; Tokio codec (RFC 5321) |
| `mailrs-smtp-client` | Outbound SMTP: MX resolution, DANE/STARTTLS, response parsing |
| `mailrs-outbound-queue` | Outbound delivery logic: DKIM signing, DSN generation, MTA-STS lookup, the retry schedule, and the queue store trait (its Postgres store behind the `pg` feature) |
| `mailrs-imap-proto`, `mailrs-imap-codec`, `mailrs-imap-format` | IMAP parser and formatter, Tokio codec, wire-format helpers (RFC 3501/9051) |
| `mailrs-rfc5322`, `mailrs-rfc2047`, `mailrs-rfc2231`, `mailrs-mime` | Message headers, encoded words, MIME parameters, the MIME body tree |
| `mailrs-mail-builder` | Outbound message builder |
| `mailrs-ical` | iCalendar (RFC 5545) and iTIP |
| `mailrs-jmap` | JMAP (RFC 8620/8621) dispatcher |
| `mailrs-sieve`, `mailrs-sieve-core` | Sieve (RFC 5228) delivery actions and interpreter |
| `mailrs-arf` | Abuse Reporting Format (RFC 5965) |

### Mail authentication and transport security

| crate | role |
|---|---|
| `mailrs-spf`, `mailrs-dkim`, `mailrs-dmarc`, `mailrs-arc` | SPF (RFC 7208), DKIM (RFC 6376), DMARC (RFC 7489) with aggregate reports, ARC (RFC 8617) |
| `mailrs-srs` | Sender Rewriting Scheme for forwarding |
| `mailrs-dnsbl` | DNS blocklist lookup (RFC 5782) |
| `mailrs-mta-sts`, `mailrs-tls-rpt` | MTA-STS (RFC 8461) and SMTP TLS reporting (RFC 8460) |
| `mailrs-tls-reload`, `mailrs-acme` | Hot-reloaded TLS certificates; ACME (RFC 8555) provisioning |

### Infrastructure

| crate | role |
|---|---|
| `mailrs-backoff` | Exponential backoff with jitter |
| `mailrs-webhook-signature` | HMAC-SHA256 webhook signing and verification |
| `mailrs-apns` | Apple push notifications over HTTP/2 |
| `mailrs-oauth-client` | OAuth 2.0 / OIDC relying-party primitives |

### The Postgres backend (not deployed)

The same core-api can be served by a Postgres-backed core instead of
fastcore. That backend is kept in the tree, compiled and tested by the
deploy gate, and not run in production. Its crates:

| crate | role |
|---|---|
| `mailrs-server` | The Postgres-backed core and the earlier all-in-one server process |
| `mailrs-core-sync` | Sync between two cores over core-api, for switching between backends |
| `mailrs-pg-dump` | Export a Postgres catalog for import into fastcore |
| `mailrs-dav`, `mailrs-postmaster` | CalDAV/CardDAV handlers and domain DNS health checks, used only by `mailrs-server` |
| `mailrs-mmalloc`, `mailrs-syscall` | An mmap-backed allocator for `mailrs-server` and its raw syscalls |

The Postgres stores in `mailrs-mailbox`, `mailrs-outbound-queue`,
`mailrs-dmarc` and `mailrs-shield` sit behind features the production
binaries leave off, so no SQL driver is linked into them.

### Test support

`mailrs-test-docker` serialises container start-up for tests that need
Postgres or Mailpit; `mailrs-perf-recipes` holds release-mode performance
budgets for compositions of crates.

## Conventions

- Crate names carry the `mailrs-` prefix; directory names do not
  (`crates/smtp-proto` is `mailrs-smtp-proto`). Workspace crates depend on
  each other by `path`.
- Configuration is environment variables only, all named `MAILRS_*`.
- The release version lives in git tags. `Cargo.toml` and
  `web/package.json` stay at `0.0.0` and the build injects the tag.
- Latency-sensitive crates carry `tests/perf_gate.rs` budgets, derived as
  described in each crate's `BUDGETS.md`; see [PERFORMANCE.md](./PERFORMANCE.md).
