# mailrs changelog

Release notes for the mailrs mail server. Format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/). Cycles are dated
using tag-push UTC dates.

Two independent tag streams: `v<major>.<minor>.<patch>` for the Rust
binary + fastcore stack, and `web-v<YYYY.MM.DD>-<seq>` for the React
web UI. Only the Rust stream is enumerated below; web releases are
tracked separately in the release-web workflow.

## Since v2.0.0 — shipped straight from `develop`

Per-version enumeration stopped at `v2.0.0` (2026-07-07). Everything
since has shipped by `scripts/direct-deploy.sh <version>`, which takes
the version on the command line, and this section kept accumulating
under a heading that still said "ships as v2.2.0" while production ran
2.85.0. The notes below are per change, newest first, with the version
it landed in where that is known.

### Host command revokes any account's agent key

- `mailrs-webapi agent-key revoke --owner <address> --id <id>`, run on the
  host, removes the key the same way the owner's own revoke does and
  records `agent_key.revoke`. An id the owner has no key under is an error.

### Send-only agent keys; a key can no longer link a sign-in method

- New agent-key scope `mail:send`: `POST /api/mail/send`,
  `POST /api/mail/send-multipart`, and `GET /api/mail/sends` narrowed to
  the sends that key made. Every other REST route answers 403 and MCP
  refuses the key. Mail sent with it goes out as the key's owner, and the
  copy is filed in the owner's Sent.
- Every send made with an agent key writes the audit line
  `mail.send.agent_key` (key prefix, `X-Caller`, source IP, Message-ID)
  before anything is filed or queued; if the line cannot be written,
  nothing is sent (503).
- `mailrs-webapi agent-key create --owner <address> --scope mail:send
  --name <name>`, run on the host, makes a send-only key for any account,
  prints the secret once and records `agent_key.create`. No HTTP route
  makes keys for another account.
- Fixed: the external sign-in callback accepted an agent key as proof of
  being signed in, so a key holder could link their own external identity
  to the key's owner and then sign in as the owner. Linking now requires a
  browser session.

### Signed-in submission refuses a null sender; lost outbound jobs are queued again

- Submission (587/465, any signed-in session) answers `MAIL FROM:<>` with
  `550 5.7.1 null sender not allowed on submission` at the end of DATA.
  Before, the message was accepted, queued with an empty sender, and the
  sender failed it as malformed without sending. Port 25 inbound still
  takes a null sender.
- The sender no longer drops a popped id silently when its claim does not
  take: the reason is logged (no row, not pending, or changed while
  claiming).
- The recovery sweep (boot and every minute) also queues again a job that
  is still `pending` but in no index, once it has not changed for five
  minutes, beside resetting stale `inflight` jobs as before.

### A large attachment can be sent

- The core's `POST /v1/users/{user}/sent` takes a body of up to the
  largest message in base64 (`MAX_REQUEST_BYTES`, from the 50 MiB SMTP
  limit) on both cores. Axum's 2 MiB default had turned away the sent
  copy of any message with an attachment of a few megabytes, and the send
  failed with an empty 500 (2.130.0).
- `/api/mail/send-multipart` answers failures in JSON: 413 with
  `limit_bytes` (25 MiB) for an oversized form, and an `error` text when
  the sent copy cannot be filed. A core request that gets no answer now
  says why (refused, reset, timed out).

### Failed outbound mail is listed, and the overview marks only recent failures

- The sender records who sent a failed job to whom along with when and
  why (`record_failure` in the outbound family); the job itself is still
  dropped.
- `GET /api/admin/queues` lists the 100 newest failed jobs as
  `status: "failed"` with `failed_at` and the reason in `last_error`. Old
  records have no sender or recipient unless the job was kept.
- `GET /api/admin/flow` adds `outbound.failed_recent`, failures in the last
  7 days. The overview's Failed stage shows that count, marked only when
  it is not zero, with the all-time total beside it; Needs attention
  lists recent failures only. Both link to the queue's Failed tab.
- The queue page has a Failed tab and a Since column (queued at, or failed
  at).

### Deleting a conversation removes every copy, and only the deleter's

- One Message-ID can sit in a mailbox several times: the Sent copy and the
  inbox copy of mail you sent yourself, or one inbox copy per alias a
  message came through. A delete now removes every file of the deleter's
  copies, not only the one the message row names. Left on disk, the others
  brought the conversation back on the next self-heal pass.
- Deleting no longer removes what other owners of the conversation still
  hold. The thread record, its message index and each message's shared
  record go with the last owner only, read from a new range index over the
  per-user message rows by thread.
- The SQL core unlinks the files of a deleted conversation too; it listed
  them and dropped the list.
- `POST /v1/admin/maintenance:backfill-message-files[?user=&dry_run=true]`
  records the copies mail already has. It reports `walked`, `added`,
  `already`, `no_row`, `messages_with_several_files` and
  `rows_without_tid`; a second run adds 0. Files with no row are counted,
  not touched.

### Deleting a read message removes its file (2.129.0)

- Deleting a conversation now also removes the maildir files of messages
  that had been read. The file was looked up under the name it had at
  delivery, which a read message no longer has, so its file stayed on
  disk after the conversation disappeared.

### Sub-addressing (2.128.0)

- Mail to `local+tag@domain` is delivered where `local@domain` would be:
  its mailbox, its alias target or its email group. The base address is
  resolved before the domain catch-all, so a tagged address of an
  existing account no longer lands in the catch-all mailbox. A tag on an
  address that does not exist still goes wherever that address would.
  The SMTP RCPT check answers the same way.
- Sending is unchanged and stays exact: `alice+tag@` is not `alice@` for
  the send-as check, and signing in with a tagged address does not match
  the account.

### Audit log shows less of other people's mail (2.126.0, 2.127.0)

- `GET /api/admin/audit-log`, its export and the `get_audit_log` MCP tool
  show the recipient of a failed send as `*@domain`, and a thread read as
  `message-id sha256:<16 hex>`. Stored rows keep the full values.
- An endpoint switched off answers 403 `{"error": "temporarily_disabled",
  "detail": ...}`; the web client shows the reason as a notice instead of
  an error with a retry.

### Listener allowlists, security alerts, and new-password rules

- Listener source allowlists, off unless configured:
  `MAILRS_IMAP_ALLOW`, `MAILRS_IMAPS_ALLOW`, `MAILRS_POP3_ALLOW`,
  `MAILRS_POP3S_ALLOW`, `MAILRS_MANAGESIEVE_ALLOW` and, in the receiver,
  `MAILRS_SUBMISSION_ALLOW` (587 and 465; never 25). Comma-separated
  addresses or CIDRs; a connection from anywhere else is closed before the
  greeting and any TLS handshake, and recorded as a `denied-by-allowlist`
  login event once per source per 10 minutes. Set but empty, or holding
  something that is not an address, stops the process at start. Production
  sets none: every port stays open to everyone.
- Security alerts, mailed only to `MAILRS_SECURITY_ALERT_TO` from
  `MAILRS_SECURITY_ALERT_FROM` (both unset: nothing is sent). Raised when an
  account is locked for unfamiliar sources (10 failures from any sources in
  an hour), a source is locked (20 failures in an hour), the server-wide
  count closes the door to unfamiliar sources (100 in 10 minutes), or one
  account signs in from 3 or more networks within 10 minutes. A source
  locked against a single account raises nothing. The same kind about the
  same account or source is raised at most once an hour; webapi mails
  what is queued as one digest at most every 5 minutes, at most 100 per
  mail. Queued alerts expire after a day.
- New passwords — set by the owner, by a reset, or by an administrator
  over REST or MCP — must have 12 to 1024 characters, must not be one of
  the 10,000 most common passwords (SecLists, MIT), and must not contain
  the account name or a domain label of three or more characters. Existing
  passwords are not checked: they keep working, sign-in is unchanged, and
  nobody is asked to change one. The web forms ask for 12 characters too.

### Login events: every sign-in attempt on record

- Every way in records one fact per attempt in the format all products
  share: `occurred_at`, `product`, `account` (as typed when it is an
  address, otherwise only its length), `source_ip`, `channel` (web,
  verify, imap, pop3, smtp, managesieve, dav, api-key), `credential`
  (password, totp, recovery-code, api-key), `result` (ok, bad-password,
  locked, mfa-required, mfa-failed), `user_agent` on the web, `key_id`
  (the key's prefix, never the key) for agent keys. Each fact is also a
  structured log line (`event="auth"`).
- Stored in the network kevy per UTC day (`auth:events:YYYYMMDD`, newest
  first) for 90 days, at most 200,000 a day; past that attempts are only
  counted (`auth:events:YYYYMMDD:dropped`). DAV and agent keys, which
  authenticate on every request, record every failure and the first
  success per account or key and source each day.
- `GET /api/admin/auth-events` (`admin.accounts`) and the MCP tool
  `list_auth_events` read them back newest first, filtered by account,
  source, result, channel and time. The web admin has a Sign-ins page
  under Oversight with the same filters.

### Requests to user-supplied addresses, and five credential fixes

- New crate `mailrs-egress`: every request the server makes to an address
  a user or a message supplied goes through it — calendar subscriptions,
  webhook deliveries, one-click unsubscribes, icon fetches, and the
  external-login key fetch. Only `http`/`https` without credentials; every
  address the name resolves to must be public (private, loopback,
  link-local and metadata, `100.64.0.0/10`, documentation, benchmark,
  multicast and reserved ranges are refused; IPv6 only global unicast), and
  the connection is pinned to those addresses; no system proxy; redirects
  only where they were followed before (icons 5, feeds 5, unsubscribe 1,
  webhooks none), each hop checked again and never from `https` to
  `http`; bodies read up to a limit (icons 256 KiB, feeds 16 MiB).
- External login verifies the `id_token`: the signature against the keys
  the issuer publishes (discovery `jwks_uri`, RS256/ES256/PS256 only),
  `iss`, `aud` (this deployment's client id), `exp`, and a `nonce` sent
  when the sign-in started and kept with the flow.
- TOTP recovery codes are stored as argon2 hashes (`recovery_hashes`, one
  per line); codes stored in plaintext before are hashed and the plaintext
  dropped the first time the account's codes are read.
- `POST /api/auth/reset-password` refuses a new password shorter than 8 or
  longer than 1024 characters with `400`, before the reset token is used;
  `change-password` applies the same range.
- Without `MAILRS_KEVY_URL` webapi authenticates nobody; the
  `X-Mailrs-User` header that named a user in that case is gone.
- A users file entry with a plaintext `password` and no `password_hash` is
  skipped and logged instead of compared.
- An agent key with no scope is refused on every route, REST and MCP
  alike; full access is the `admin` scope, named. (Keys made before scopes
  were required were the only ones without.)
- SMTP `AUTH LOGIN` accepts the username on the `AUTH` line (RFC 4954
  §4). It used to ask for the username again and read the password as
  one, so such clients failed to sign in.
- A sign-in name that is not an address is never logged or stored: logs
  show only its length and the login guard keys on its hash. A password
  typed into the user field had been written to the logs as the account.
- `MAILRS_TRUSTED_PROXIES` in production is the docker gateway that
  docker-proxy connects from (`172.18.0.1/32`), not every private range;
  webapi logs an error once per address when a private address it does not
  trust sends `X-Forwarded-For`.

### One login guard for every way in

Failed logins were counted only on SMTP submission and DAV, per source
and account, each in its own store; changing source started the count
again, and IMAP, POP3, ManageSieve, the web login and `/api/auth/verify`
counted nothing.

- Web login (password and TOTP step), `/api/auth/verify`,
  `/api/auth/verify-totp`, DAV, IMAP, POP3, ManageSieve and SMTP
  submission all ask one guard before the password is checked, and report
  the outcome after. State is in the network kevy (`ag:` keys), so a
  failure on one protocol counts on all of them, in every process.
- Four counts, each a sliding window: source and account (5 in 15
  minutes, locks 30 minutes, doubling), source (IPv4 address or IPv6 /64;
  20 in an hour, locks an hour, doubling), account from any source (10
  in an hour, locks 15 minutes, doubling up to 2 hours), and everything
  (100 in 10 minutes, closes the door while the window stays full).
- A source the account signed in from within 30 days (IPv4 /24, IPv6
  /64, the 20 most recent) is familiar and only the source-and-account
  count applies to it, so guessing at an account from elsewhere does not
  lock its owner out.
- A locked attempt is refused without verifying the password: `429` with
  `Retry-After` on HTTP, `NO [UNAVAILABLE]` on IMAP, `-ERR [SYS/TEMP]` on
  POP3, `NO (TRYLATER)` on ManageSieve, `421 4.7.0` on SMTP, each saying
  how many minutes to wait and not which count tripped.
- When the store cannot be reached the attempt is let through and an
  error is logged (`event="auth_guard_unavailable"`).
- The web client address is the last `X-Forwarded-For` hop only when the
  connection comes from `MAILRS_TRUSTED_PROXIES` (CIDRs; loopback when
  unset); otherwise it is the connection's own address.
- `/api/auth/verify` answers `{"valid":false,"reason":"invalid_credentials"}`
  for an unknown account and a wrong password alike, never compares a
  stored value that is not an argon2 hash, and takes an optional
  `client_ip` so the calling service can have the person counted rather
  than itself. `/api/auth/verify-totp` takes the same field. SMTP
  submission no longer compares a stored non-hash either.
- `POST /api/auth/logout` ends a session sent as `Authorization: Bearer`
  as well as the cookie one, and drops it from the owner's index.
- Agent keys: no key, of any scope, can list, create or revoke keys
  (`/api/agent/keys`); that takes a signed-in session. A new key must
  name at least one scope; the web form's "Full access" now sends
  `admin` and the form starts on "Mail read + write". Keys made earlier
  with no scope keep working.
- Sending: the send-as check compares the address inside `Name <addr>`,
  so a display name is allowed on any address the account may send as,
  and the name is written quoted (or RFC 2047 encoded) into the From
  header. A `from` containing a line break is refused with `400`.

### Local forwards: an account's mail copied into another account here

An account that exists only so a service can sign in with it (sentori@,
insight@) still receives replies and bounces, which went unseen once the
address stopped falling through to the catch-all. A local forward copies
everything delivered to the account into other accounts' mailboxes.

- The copy is written straight into each target's maildir by the spool
  drain, through the target's own sieve, under the fraud verdict the
  message already has. Nothing is queued, sent over SMTP or rewritten
  with SRS, as a sieve `redirect` would be.
- One hop: a forwarded copy is not forwarded again, so two accounts that
  forward to each other cannot loop. A target that is not a mailbox here
  is skipped and logged; the account's own copy is never lost to a
  forward that fails.
- Copies to email-group members are forwarded the same way.
- `GET` / `PUT /api/admin/accounts/{address}/forwards` and the MCP tools
  `get_account_forwards` / `set_account_forwards` (`admin.accounts`,
  audited as `account.forwards`); targets must be other accounts. Stored
  in the network kevy (`mailrs:forward:local:{address}`), so both cores
  deliver alike.

### Submission checks the sender; accounts can be granted send-as addresses

Signed-in SMTP submission (465 / 587) used to accept any `MAIL FROM` and
any `From:`: one guessed password was enough to send as any address from
this server. It now applies the rule the web send always had.

- A signed-in account may send as its own address, the aliases pointing
  at it (a domain catch-all does not count), the email groups it belongs
  to, and the addresses an administrator granted it; a super-admin as
  anyone. The envelope sender and the `From:` header are both checked
  once the message is in; an empty `MAIL FROM:<>` passes. Anything else
  is refused with `550 5.7.1`, and `451 4.3.0` when the account's
  permissions cannot be read.
- Every decision is logged as `event="submission_sender"` with the
  account, both addresses, the client address and, for a refusal, which
  of the two failed. `MAILRS_SUBMISSION_FROM_CHECK=log` keeps logging
  without refusing; unset or anything else refuses.
- A namespace's sandbox login may only use its own namespace's
  addresses; a login known only to `MAILRS_USERS_FILE` only its own.
- `GET` / `PUT /api/admin/accounts/{address}/send-as` and the MCP tools
  `get_account_send_as` / `set_account_send_as` read and replace an
  account's grants (`admin.accounts`, audited as `account.send_as`).
- The kevy core now counts aliases and email groups in `send_as`, as the
  SQL core did; before, only a super-admin could send from an alias in
  the web UI. The SQL core reads email groups from the network kevy, as
  delivery does, and no longer caches `send_as` for five minutes, so an
  alias change applies at once. Its grants are in `account_send_as`
  (`migrate-052`, which also indexes `aliases(target_address)`).

### fastcore's core API asks for its secret; the image proxy is gone

On 2026-10-24 a signed-in client used `GET /api/proxy/image?url=` as a
way into the host: the handler fetched any `http(s)://` URL from inside
the webapi container and returned the body, and fastcore's core API on
`:3301`, reachable from every container, checked no credential. Together
they let any session read any account's mail through the core API.

- `/api/proxy/image` is removed. The web client stopped using it when
  mail HTML moved into a Shadow DOM, which loads images directly.
- fastcore puts every route except `/v1/healthz` and `/v1/readyz` behind
  `Authorization: Bearer <MAILRS_CORE_API_SECRET>`, as the SQL core
  already did. webapi and receiver send it; an empty secret leaves the
  routes open for tests and local runs, with a warning at start.

### Senders nothing can stand behind are held for review

Two rules in the shipped bundle, both hold-worthy:

- `reserved-sender-domain` — a `From:` under `.invalid`, `.test`,
  `.example`, `.localhost` or `example.com/.net/.org`, from outside.
  Those names cannot receive mail, so nobody who wants an answer writes
  from them.
- `nothing-vouches-for-the-sender` — the `From:` domain publishes DMARC,
  the message carries no DKIM signature at all and SPF says `fail`,
  `softfail`, `neutral` or `none`. A signature that failed to verify, or
  an SPF `temperror`, does not count.

Over 43,929 production messages they add 40 holds and release none: loan
and compensation scams, purchase-order lures, a password-expiry phish,
brand impersonations, and a tester's probes from
`probe@nodmarc-probe.invalid` and a forged `ceo@gmail.com`.

`mailrs-fraud-check compare` now reads SPF, DKIM and DMARC off the stored
`Authentication-Results:`, and whether the receiver's own stamp marks the
unauthenticated path (with `MAILRS_HOSTNAME`), as the production sweep
does. Before, it left them empty, so a rule that reads them measured
zero offline while it held mail in production.

### A letter asking for the reader's LINE QR code is held for review

The business-email compromise impersonating this company (`齋藤 真`,
`富川 貴司`, `西口 征郎`, signing as `GOLIA株式会社` or `ギリア株式会社`)
reached an inbox again on 2026-10-23, from `ptvmfekd@nogz.cn` with SPF,
DKIM and DMARC passing. No rule had anything to read: the domain's name
is four letters, too short for `minted-address`, and the mailer's
version field was a bare build number the `X-Mailer` rule does not take.
What every wave keeps is the request — send your personal LINE QR code
to this address — and the money is asked for later, in the chat.

New rule `asks-for-the-readers-line` (content, 6.0, holds), on the new
fact `asks_for_the_readers_line` (`mailrs_fraud::line_handoff`): within
one sentence, a word for the reader then LINE then a QR code, or LINE
then a QR code then "please send / reply". Measured over 43,838
production messages: 263 matched, all this campaign, nothing else; 17
were held by no other rule, and none is released. Shop newsletters that
print a LINE QR code to scan, and a sender offering their own, ask for
nothing and are not convicted.

### The SQL core keeps a body in its mailbox's folder

The SQL core had two ideas of where a message's file was. Delivered and
sent mail went into the mailbox's Maildir++ subfolder (`.Sent/…`), while
IMAP APPEND wrote every message into the user's top folder, COPY and
MOVE only touched rows (so two rows shared a file, and EXPUNGE left the
file behind), and the IMAP reader looked only in the top folder. A row's
`maildir_id` is now the body's blob ref, as fastcore stores it — the
file name for INBOX, `.Name/<file>` for any other mailbox — so mail
synced from fastcore and mail written here are read the same way, by
this core and by webapi. APPEND writes into the mailbox's folder, COPY
writes a file of its own into the target, MOVE moves the file, EXPUNGE
removes it, and every reader resolves the blob ref with
`mailrs_maildir::locate`.

### A message moved to another folder stays read

IMAP COPY and MOVE on fastcore wrote the copy into the target folder's
`new/` with no flags, so a read message moved to Archive came back
unread (and unflagged). The copy now carries the source's flags in its
file name and in the index, as APPEND already did.

### The all-in-one server is gone

`crates/server` used to build two binaries: `mailrs-pg-core`, the SQL core
that can take fastcore's place, and `mailrs-server`, the single process
that preceded the four-process stack and was in no image. `mailrs-server`
is deleted, and with it everything only it ran: its web tier and MCP
server (production's REST and MCP are `mailrs-webapi`, which serves either
core), the SMTP listeners, outbound delivery, the RBL monitor, the DMARC
aggregate builder, the Postgres webhook, calendar-feed, OIDC, TOTP and
API-key stores, the conversation cache, and the role switch that told the
two binaries apart. The crates only it linked — `mailrs-mmalloc`,
`mailrs-syscall` and `mailrs-postmaster` — are removed from the
workspace, as are the old conversation-list and search queries in
`mailrs-mailbox` that only its web tier called.

`crates/server` is now the SQL core alone, and `mailrs-pg-core` builds
without a feature flag: `cargo build -p mailrs-server --bin
mailrs-pg-core` for PostgreSQL, `--features spg` for the embedded engine.
`scripts/build-pg-core.sh` takes `--features spg` or nothing.

### `Hao` — a subject cut from the reader's own address (2.87.0)

A cold-sales campaign sends mail whose subject is one word: a piece of
the recipient's mailbox read as a first name — `Hao` to
`lihao@golia.jp` — with a body opening `Hao, would 4 to 8 more
high-paying contracts … move the needle?`, from a domain registered for
the purpose. 21 of 39,685 production messages have that subject shape
and all 21 are this campaign. The new rule `subject-cut-from-the-address`
scores 5 — the Junk threshold — and never holds: it is cold sales, not
fraud on its face.

A scored rule used to reach only new mail. The historical sweep acted on
hold-grade findings and nothing else, so everything delivered before a
scored rule existed stayed where it was. It now also moves a conversation
nothing holds, whose fraud score alone reaches the threshold, to Junk —
the sum the receive path already uses — and the backfill journal counts
them as `junked`. Run first on a local copy of production: 37,018
conversations, 29 scored into Junk (16 of this campaign, 13 Amazon /
Apple / JCB phishing the brand rules score at 7.5–9), 18 moved and 11
already there; no hold added or released.

### A sender with no avatar at all

`Microsoft Rewards` from `customeremail.microsoftrewards.com` drew no
avatar — not the letter fallback, nothing. The icon cascade asks
Google (404 for that domain) and then DuckDuckGo, which answers `200
image/x-icon` with **43 bytes**: a 1×1 transparent GIF, its way of
saying it has no icon. Any 200 counted as a hit, so the UI stretched
one transparent pixel into a 36 px circle.

The cascade now measures what it got — PNG, GIF, ICO and JPEG state
their size in the header, SVG (BIMI) is taken as-is — and treats
anything under 8×8 as the provider saying no, carrying on to the next
source and finally to the letter avatar. Hits live in kevy for a week,
so the same test runs on the way out of the cache: placeholders
already stored are dropped on first touch instead of lingering for
seven days.

### The sweep can tell how a stored message was submitted

`claims-our-domain` needs to know that a message arrived on a session
that did not authenticate, and the historical sweep had no way to know
— so it declined, and mail already delivered could never be re-judged
by it.

It can now read it off the message: the receiver stamps
`Authentication-Results:` on the inbound path only, which runs only
for unauthenticated sessions, and prepends it. A stored message whose
**first** such header names this deployment came in as a stranger; one
our own people submitted carries none of ours. That needs
`MAILRS_HOSTNAME` on the fastcore process, which only the receiver had
— now in its compose block. Unset, the sweep says so in its log and
the rule keeps declining, the same shape as the missing org names.

### Mail claiming our own domain is held for review

A BEC message from `aiyhccspbu@golia.jp` — display name `齋藤 真`,
subject `ギリア株式会社 業務変更`, asking the reader to reply with
their personal LINE QR code — reached the inbox with a "Suspicious
sender" badge and nothing else. Every hold-worthy identity rule starts
at `external(m)`, which is false for our own domains: the exemption
written for outsiders claiming our *name* also excused outsiders using
our *domain*. `minted-address` could not fire either, because `golia`
is a word somebody chose.

New rule `claims-our-domain` (identity, 6.0, holds). The fraud scan
runs only on sessions that did not authenticate — our own people submit
authenticated — so a `From:` at one of `MAILRS_LOCAL_DOMAINS` there is
somebody outside saying they are us, which is also what
`_dmarc.golia.jp` (`p=quarantine; sp=reject`) already says to do with
it. Domains on the org-name allow-list stay exempt.

It needs a new fact, `unauthenticated`, which the receiver sets and the
historical sweep leaves false — so a rescan, which cannot know how a
stored message was submitted, can never hold the mailbox's own internal
history. That also means this catches new mail only; nothing already
delivered moves.

Not covered: the auth facts (`spf` / `dkim` / `dmarc`) are exposed to
the rules but still never filled, because the fraud scan runs before
the pipeline stage that computes them. A rule keyed on authentication
is not expressible until that order changes.

#### Corrected in v2.86.0 — it held 62 of this deployment's own mail

The paragraph above is wrong in the way that matters: "our own people
submit authenticated" is not true of this deployment's own services.
They submit to the MX without SMTP AUTH, from the container bridge, and
some of them are unsigned — `devops@` (38 conversations), `alias-verify@`
(36), `noreply@`, `qa@`, `postmaster@`, all `spf=softfail dkim=none
dmarc=fail`. Of 108 messages in that class, three were the real thing.
The sweep did hold them too: by then it read our own
`Authentication-Results` stamp back off the stored message, so
"unauthenticated" was answerable historically after all, and the
activation's backfill applied it to 36,676 threads.

Rolled back to the previous bundle first (the backfill released all 63),
then the rule was given the two facts that can tell our own mail from a
forgery:

* **where the connection came from.** Our services reach us at
  172.18.0.1; a forger cannot borrow our own host's address. The
  receiver reads it off the socket, the sweep off the first `Received:`
  line it wrote, and a message with no line to read counts as outside.
* **what alignment said.** `dmarc == 'fail'`, not `~= 'pass'`:
  production has mail from a public address that IS us and proves it
  (`spf=pass dmarc=pass`), and a rule firing on "not proven" would hold
  that too. Unknown declines.

The receiver could not answer the second one at all — it scans before
the pipeline stage that checks alignment — so it now scans again
afterwards, and that pass is the verdict `ingest.rs` holds a
conversation by. The accept/junk/reject decision above it is made
before, and is not revisited.

The rule's tests are three real production shapes copied out of the
maildir: the reported BEC is held, the control plane's own mail is not,
and neither is the verified public sender at our domain.

## v2.2.0

Rust-side kevy 3.17 network-op adoption + admin-panel data-source
repair. Sits on top of the shipped `v2.0.0` GA and `web-v2026.07.08-1`
web release; v2.1 web work is complete and covered further down.

### kevy 3.17 network path (Phase 1-3)

kevy team shipped `kevy-client 1.14.0` on 2026-07-08 with the wraps
that were gating every network-side kevy 3.x adoption path —
BRPOP / BLPOP / BZPOPMIN / HEXPIRE / HPEXPIRE / HPERSIST /
ZINTERSTORE-with-weights / IDX_* / FEED_READ / pipeline. Workspace
lifted 1.13 → 1.14 (API-compatible, no code churn).

- **`mailrs-fastcore-sender`** — main loop was
  `rpop(PENDING_KEY, 1) + sleep(cfg.poll_ms.max(500))` polling.
  Migrated to `brpop(&[PENDING_KEY], Some(5s))` on a
  `spawn_blocking` thread. Queue-arrival wake-up: `~poll_ms/2` avg
  → **0 ms**; idle CPU: 2 kevy RTTs per `poll_ms` → **~zero**;
  wake-up floor unchanged on a hot queue. Commit `4f073eb6`.
- **`mailrs-fastcore::bounce::spawn_bounce_drain`** — outer
  `sleep(10 s)` between `drain_once` invocations replaced by an
  in-drain `brpop(&[BOUNCE_PENDING], Some(10 s))` on the first
  pop, followed by non-blocking `rpop` for burst throughput.
  Bounce delivery latency: uniform 0-10 s → **0 ms** on hot,
  timer-bounded on idle. Commit `15f953cc`.

Phases 4-8 status after this cycle's audit:

- Phase 4 · HEXPIRE for sidecar TTL — **N/A**. Every `HSET +
  EXPIRE` pattern in webapi expires the whole hash key, which is
  normal Redis usage, not the sidecar-key antipattern kevy team
  warned about. No `<key>:expires_at` sidecar keys grep in the
  codebase.
- Phase 5 · ZINTERSTORE for webapi conversation-list filter —
  **already done**. webapi's list handler delegates to fastcore
  RPC; fastcore's `list_threads_by_activity` in
  `mailbox-kevy/src/list_threads.rs` already uses `zinterstore`
  (landed v1.9.4 Stage B.6).
- Phase 6 · idx_query for admin CRUD — **deferred**, needs a
  data-model migration to hash-field entries.
- Phase 7 · feed_read for WS bridge — **deferred**, no
  user-visible regression from the current pubsub broadcast.
  WS clients refetch full state on reconnect, so lost pubsub
  events between webapi restarts self-recover.
- Phase 8 · core-sidestate 4-shard `atomic()` risk — **blocked**
  on the kevy team's internal `docs/KEVY_ADOPTION_GUIDE.md`
  (`atomic_all_shards` pattern; note flagged this in the
  2026-07-08 response).

### Admin data-source repair

Session-observed regressions from the fastcore-split /
network-kevy-alias flip: several webapi admin handlers still
walked legacy pre-split keys that were emptied at cutover, so the
admin UI rendered empty state for a super-admin caller.

- `handlers::admin::list_aliases` / `add_alias` / `remove_alias`
  → route through fastcore `state.core.list_local_aliases` /
  `upsert_local_alias` / `delete_local_alias`. `id` field is a
  deterministic i64 hash of `source_address` (JS safe-integer
  bounded) so the frontend's delete-by-id round-trip works
  without a schema break. Commit `a23a2cf3`.
- `handlers::complete::audit_accounts` → route through
  `state.core.list_accounts` (fastcore embedded kevy). Populates
  the `AuditAccount` shape from `AccountWire`. Commit `a23a2cf3`.
- `handlers::admin::list_domains` / `add_domain` / `remove_domain`
  → route through fastcore `state.core.list_domains` /
  `add_domain` / `remove_domain`. Unblocks the alias-create form's
  domain dropdown (it was a required field, so empty dropdown
  meant no aliases could be added through the UI). Commit
  `b1fa90f5`.
- **Domain-index self-heal.** `fastcore::add_account_route`
  auto-`upsert_domain(a.domain)`; webapi `handlers::admin::add_alias`
  auto-`state.core.add_domain(domain_from_source)`. Prior gap:
  neither path touched the domain index, so a fresh domain was
  invisible in the UI until manual `POST /admin/domains`. Failure
  swallowed with warn — the account/alias write is authoritative,
  the domain index is a UI derived view. Commit `2994f700`.

### Web-only fixes bundled into this cycle

Session-observed:

- Dashboard "Unread" badge was reading
  `data.stats.unread_messages` (`/mail/stats`) with a fallback to
  `data.folders.INBOX.unseen` (`/mail/folders`) — both server-side
  aggregates that don't invalidate on mark-read. Optimistic
  `patchAllInfiniteLists` only touches `conversationKeys.list()`,
  so the badge stayed stuck at the pre-mutation count for the 60 s
  `REFRESH_INTERVAL`. Derive from `data.conversations` instead
  (same cache line the mutation patches). Semantic is now
  thread-count-of-unread, matching `useCurrentUnreadCount`. Commit
  `a69420b6`.

## Unreleased-web — accumulating on `develop`, ships as **web-v2.1.0**

Web-side architectural redesign begun 2026-07-07. The Rust
binary + fastcore stack landed as `v2.0.0` on 2026-07-07; this
section covers the web-only redesign that ships next.

### Web v2.1 — Phase 7-10 (Zod wire boundary complete)

Phase 10 closes the wire migration:

- **Zod at every wire boundary.** Every network call now
  Zod-parses its response — no `as any`, no raw JSON out of
  `fetch`. Coverage: 0 → ~165 Zod-guarded `wireFetch` callsites.
  100 % of the mail hot path, 100 % of auth, 100 % of user
  settings CRUD, 100 % of admin CRUD (12 resources × ~4 endpoints
  each) all routed through `wire/endpoints/*`.
- **Uniform admin CRUD.** `wire/endpoints/admin.ts` provides
  `adminListGet<T>` / `adminObjectGet<T>` / `adminPost<T>` /
  `adminPut<T>` / `adminPatch<T>` / `adminDelete` — Zod-parses
  the envelope (list-vs-object shape) but keeps content
  permissive so downstream keeps its cast contract. 101 admin
  callsites bulk-migrated.
- **Multipart send + inline upload.** `wireFetch` gained
  `bodyRaw?: BodyInit` to carry `FormData` through the same
  Zod-validated response path. `wireSendMailMultipart` +
  `wireUploadInlineImage` replace two raw `fetch('/api/...')`
  callsites in `send-mail.ts` and `rich-editor.tsx`.
- **ESLint boundary gate.** `no-restricted-imports` blocks new
  callers from `postJson` / `putJson` / `deleteJson` /
  `fetchJson` / `fetchList` from `@/lib/api` outside of
  `src/wire/**` and the shim itself. New raw-fetch = build
  failure.
- **`lib/api.ts` collapsed to a domain-adapter shim.** The
  remaining exports (`snoozeConversation`,
  `unsnoozeConversation`, `recordFeedback`, `saveDraft`,
  `deleteDraft`, `toggleReaction`, `getThreadReactions`,
  `listDrafts`) delegate to wire adapters via lazy import.
  Raw `postJson` / etc. helpers survive only because the
  shim + its co-located tests reference them.

Real bugs surfaced by Zod alignment during Phase 10:

1. **`TotpSetup.qr_url` was fabricated** — backend actually
   returns `otpauth_url` (`crates/webapi/src/handlers/complete.rs::
   totp_setup` line 336). The frontend type in `_shared.tsx`
   invented `qr_url`, so the QR display paragraph was silently
   rendering `undefined` for the entire life of the TOTP-setup
   panel. Type + render site both switched to `otpauth_url`.
2. **`OidcClientConfig` shape mismatched backend.** Frontend
   expected `{enabled, login_url, provider_name}` but backend
   sends `{enabled, providers[]}`. Schema now accepts both;
   UI reconciliation deferred (OIDC login isn't the primary
   sign-in path).
3. **`change_password` field name.** Frontend was passing
   `old_password`; backend expects `current_password`
   (`ChangePasswordRequest` line 246). Adapter aligned.
4. **`ReactionSummary.reacted` was fabricated in test fixtures.**
   Wire truth is `me: boolean` (`crates/webapi/src/handlers/mail.rs::
   get_thread_reactions` line 163). Test fixtures updated; adapter
   aligned with `types.ts::ReactionSummary`.
5. **`render-preview` returned a different shape** than the
   frontend expected — backend actually emits `{png_base64,
   fallback_html?}`, frontend fabricated `{previews[],
   errors[]}`. Panel was silently broken. Schema is permissive
   (`passthrough`) for now; UI reconciliation queued.

Phase 7 (design-token discipline):

- **Motion tokens** (`--duration-fast/base/slow`,
  `--ease-standard/emphasized`, `--animate-slide-up`) —
  bottom-sheet + context-menu now use `animate-slide-up`
  instead of inline `animate-[slideUp_200ms_ease-out]`.
- **Elevation tokens** (`--shadow-elevation-0..3` with dark-mode
  overrides). Adoption is incremental — legacy
  `shadow-{sm,md,lg,xl,2xl}` callsites (14 spots) stay until
  touched.

Phase 8 (route data-router):

- **Route-level error boundary.** `RouteErrorFallback` renders
  under every route branch (`errorElement`) — a thrown
  `WireErrorException` (auth / server / validation / network),
  render crash, or 404 shows a specific message with a targeted
  action (sign-in for auth-expired, reload for network, home
  for anything else), replacing the browser's grey unstyled
  reload prompt.

Deferred to a follow-up (design decision needed):

- Route loaders for `/mail` + `/` that call
  `queryClient.ensureQueryData()` — needs a decision on
  whether the loader reads initial filter state from URL params
  or from a jotai store snapshot.
- Full `shadow-{sm,md,lg,xl,2xl}` → `shadow-elevation-*`
  migration.
- Skeleton CLS audit — requires DevTools Performance Insights
  runs, not scriptable.
- Cross-screen mutation matrix regression tests (dashboard ↔
  mail-list ↔ thread-view × mark-read / star / archive / etc.)
  — the mutation-callsite convergence in Phase 10 makes this
  primarily a manual QA pass rather than an automated matrix.

### Web v2.1 — Phase 1-6 shipped 2026-07-07 → 08

- **RQ single-source-of-truth.** The mail list, dashboard, and thread
  view now share one cache line
  (`conversationKeys.infinite(filter)` +
  `conversationKeys.list(filter)`). Any mutation invalidates the
  entire prefix, so a mark-read on `/mail` immediately reflects in
  the home-page unread badge — the original user-reported bug that
  triggered the redesign.
- **Deleted atoms.** `conversationsAtom`, `threadMessagesAtom`,
  `unreadCountAtom`, `initialLoadingAtom`, `hasMoreAtom`,
  `loadingMoreAtom` are gone. Jotai now holds only genuine local-UI
  state (selection, batch mode, mobile view, filter chips, sort
  order, compose source). `store/chat.ts` renamed to
  `store/ui.ts` to reflect the new scope.
- **New primitives.**
  - `hooks/use-flat-conversations.ts::useFlatConversations(filters)`
    — identity-preserving flatten over
    `conversationKeys.infinite(filter)`.
  - `hooks/use-current-mail-filters.ts::useCurrentMailFilters`,
    `::useCurrentThreadMessages`, `::useCurrentUnreadCount` —
    the ONE way to compose atoms into a `MailListFilters`, read the
    open thread's messages, or derive an unread total. All go
    through RQ.
  - `reducers/snapshot.ts::patchAllInfiniteLists(qc, updater)` —
    walks every `conversationKeys.infinites()` cache line and
    applies a pure updater; the sole primitive for optimistic
    patches (used by the 6 keyboard shortcuts + mark-all-read + the
    reducers in `reducers/commands/conversation.ts`).
  - `wire/client.ts::wireFetch<T>(schema, req)` — the only fetch
    wrapper. Uses Zod schemas at the wire boundary for runtime
    validation.
  - `domain/ids.ts` branded ID types (`ThreadId`, `MessageId`,
    `Uid`, `AccountId`, `DraftId`, `DomainName`, `AliasAddress`)
    with narrow constructors enforced at wire parse.
- **Anti-flash discipline.** Global `queryClient` defaults include
  `placeholderData: keepPreviousData` — filter changes and page
  transitions never blank the screen.
- **Design tokens.** Three sub-`text-xs` sizes (`text-tiny` = 10 px,
  `text-mini` = 11 px, `text-mid` = 13 px) formalised in
  `index.css`; 34 inline `text-[Xpx]` literals migrated. Total
  arbitrary-value literal count: 99 → 65 (remaining are all
  documented legitimate uses — CSS-variable references, viewport
  units, animation keyframes, and `pctToWidth` bucket lookups).

### Locked RFC decisions

1. Zod runtime validation at the wire boundary.
2. `react-router` v7 data routes via `createBrowserRouter` +
   `RouterProvider` (Phase 8 — pending).
3. `openapi-typescript` output generated at build time from
   `web/public/openapi.json`.
4. `syncStoragePersister` for React Query cache persistence to
   localStorage.
5. Env-only feature flags via `__FLAGS__` vite define.

### Deployment protocol during this cycle

Per user directive 2026-07-07: every phase built locally and
direct-rsync'd to prod + staging, no CI. Only the final v2.1.0 tag
runs through `release.yml` on explicit user go-ahead.

## v2.0.0 — 2026-07-07

GA release of the Rust binary + fastcore stack. Landed at commit
`4f01bc64`. Consolidates every Stage B / C / D increment that had
been accumulating on `develop` since v1.9.4.

- **Stage B.6 (v1.9.4 landed 2026-07-06):** ZINTERSTORE materializes
  multi-filter conversation lists so combined predicates (inbox ∩
  has_unread, starred ∩ archived) return an exact intersection, not
  the highest-priority single index. Per-request temp key with 60 s
  orphan-TTL + post-use del.
- **Stage B.7 / B.8:** kevy 3.17 change feed replaces IMAP IDLE's
  tokio broadcast::channel — durable across restarts, no lost events
  under slow-consumer lag. `Store::changes_since(gen, offset)` with
  500 ms poll cadence. B.7 (idx_create) skipped because our alias /
  domain / account data model uses plain string keys + set indexes,
  not hash-field entries; a data-model migration would be required
  and belongs in a separate RFC.
- **Stage D · G12.5:** `GET /api/admin/audit-log/export?since=&until=
  &actor=&action=` — unrestricted-scan JSON export for bulk retention
  offloading. `AuditQuery` gains `since` / `until` time-window fields
  used by both list + export handlers. Existing 50 K row count-cap
  retention retained — more robust than time-window sweeps under
  bursty load.
- **Stage D · G13.3:** `POST /api/scheduled/{id}/cancel` +
  `/reschedule` — outbound queue control on the SCHEDULED zset.
  Sender-verified; reschedule enforces future timestamp; sender
  mismatch returns 404 to prevent id enumeration.
- **Stage C.1:** MCP tool surface expanded from 37 to **62** tools
  across 10 v2 batches — hits the plan target:
    - Batch 1 (admin-read): `list_groups`, `list_apps`,
      `list_email_groups`, `list_greylist_local`, `list_aliases_admin`.
    - Batch 2 (admin-misc): `reconcile_maildir`, `list_scheduled_outbound`,
      `get_email_group_members`.
    - Batch 3 (per-user outbound control): `cancel_scheduled`,
      `reschedule_scheduled`.
    - Batch 4 (self-introspection): `get_my_permissions`,
      `list_own_scheduled`.
    - Batch 5 (encryption keys): `list_own_encryption_keys`,
      `get_public_key_of`.
    - Batch 6 (admin queue): `list_admin_queue`, `list_failed_outbound`.
    - Batch 7 (server info + retry): `get_server_info`, `retry_queue_message`.
    - Batch 8 (thread summary): `get_thread_summary`.
    - Batch 9 (thread mutations): `snooze_thread`, `unsnooze_thread`,
      `pin_thread`, `unpin_thread`, `dismiss_thread_action`.
    - Batch 10 (dashboard metrics): `get_inbox_metrics`.
- **Stage C.5 (partial):** `mcp.rs` → `mcp/{mod,params,tools_v2_batchN}.rs`
  named-router split. Each new batch file <500 lines (file-size
  hard rule); the 37 legacy tools remain in mod.rs pending a
  post-v2 per-category split.
- **Upstream tracking:** kevy-client 1.13 does not wrap kevy-server's
  3.17 features (brpop / hexpire / zinterstore / idx / changes_since).
  Phase 4 (BRPOP), Phase 5 (HEXPIRE), and Phase 7/8 for network paths
  remain blocked.
- **Docs / rules:** ARCHITECTURE.md fastcore-topology refresh + crate
  count 44 → 59, README.md dropped legacy `docker compose up postgres
  kevy` (both engines in-process since v1.7.95), PERFORMANCE.md
  added a v2 kevy 3.17 refactor row with a per-site table and the
  staging soak `slow_pct` trend (0.72 % → 0.59 % → 0.67 %),
  `web/public/openapi.json`
  version 0.9.3 → 2.0.0, CHANGELOG.md (this file) established.

## v1.9.4 — 2026-07-06

- Stage B.6 · ZINTERSTORE — see Unreleased.

## v1.9.3 — 2026-07-06

- Stage B.3 · N+1 read fanout collapsed into atomic snapshot closures.
  `list_threads_by_activity` / `list_thread_messages` on mailbox-kevy;
  `search.rs` linear-fallback consolidated from up to 500 kevy_client
  Connections down to one.

## v1.9.2 — 2026-07-06

- Stage B.2 · Atomic counters. `allocate_uid` + `register_uid` +
  `uidvalidity` collapse read-check + INCR + rev/forward index write
  into a single AtomicCtx closure. 100-thread same-mid idempotency
  regression test added. Three duplicate `next_id` helpers
  (complete/prefs/admin) replaced by a bare `c.incr()`.

## v1.9.1 — 2026-07-06

- Stage B.1 · 10 mailbox-kevy CRUD methods + `ingest_delivered_file`
  self-heal + server session table + auth 2FA recovery-code all
  collapse multi-op RMW into single `store.atomic(|ctx| ...)`
  closures. kevy 3.17 `AtomicCtx` gained `zrem` / `hdel` / `del` /
  `sadd` / `srem`, retiring the 1.15-era two-step workarounds.

## v1.9.0 — 2026-07-06

- **Foundation for v2.0.0:** kevy-embedded 1.15 → 3.17.2 + kevy-client
  1.12 → 1.13.1 workspace lift. Zero source-level breaking changes —
  core op signatures identical between 1.15 and 3.17.
- 1.x AOF forward-compat proven: prod 531 MB / 3.7 M-command AOF
  replays clean on the 3.17 binary in 1.68 s (dbsize=84708, 40
  aliases intact).
- Compose consolidation: root `docker-compose.{prod,prod.split,}.yml`
  deleted (legacy monolith duplicates); canonical files live under
  `deploy/`.
- kevy container pinned `latest` → `3.17.1` in prod / staging / split
  composes.
- CI: `STAGING_GATE_GRACE` cleared to `__none__` sentinel so v2 tags
  never bypass the staging soak gate. `release-web.yml` dead
  `up -d mailrs` step (targeted the pre-fastcore monolith service)
  removed. `scripts/staging-fastcore-up.sh` parametrized IMAGE_TAG
  via `MAILRS_IMAGE_TAG` env.

## v1.8.11 — 2026-07-06

- MCP tool port batch, alias case-sensitivity fix, misc fmt.

## v1.8.5 – v1.8.10 — 2026-07-05 / 06

- Alias recovery lineage: case-sensitivity in `resolve_alias`
  (byte-eq cycle detect), AliasStore trait abstraction, network kevy
  backend flip, mobile-mail conversation-panel bleed fix.

## v1.8.0 – v1.8.4 — 2026-06 / 07

- fastcore 4-process split arrives in prod. receiver + fastcore +
  webapi-fc + fastcore-sender + shared kevy container. `SPG` lane
  retained on staging as the pg-core dogfood dual-mode partner.

## v1.7.x — 2026-05 / 06 / 07

- Original monolith + spg-dogfood iterations. Notable rollouts:
  v1.7.95 kevy embedded cutover, v1.7.132 web bind-mount rollout,
  v1.7.148 SPG cutover, v1.7.170 prod livelock hotfix, v1.7.180
  final baseline before v1.8.

## Earlier

- v1.6 and earlier are covered by GitHub Releases only.
