# Architecture — Stones and Cement

> 一个个开源 crate 或必要的闭源 crate / lib 就像是一个个石头，
> 其他特化的业务代码就是水泥，
> 水泥把石头缝填满就是我们的坚固的建筑。
>
> — direction, 2026-05-23

This document codifies the architectural lens that drives mailrs's
modular decomposition. **Use it as a self-check whenever evaluating a
piece of code, considering a new module, or auditing for dedup.**

## The lens

Every line of code is either a **stone** or **cement**.

### Stone (石头) — generic, reusable, publishable

Stones are the load-bearing primitives. They have:

- **A single identity expressible in one sentence.** "RFC 5322 parser."
  "Exponential backoff with jitter." "ClamAV INSTREAM client." If you
  can't pin the identity in one line, it's not a stone — it's a bag.
- **An RFC, a well-known concept, or a single algorithmic primitive
  as the boundary.** Stones are bounded by externally-meaningful
  contracts, not by where the team happened to split files.
- **Zero (or minimal, justified) coupling.** A stone doesn't pull in
  the project's data model, the project's PG schema, the project's
  config object. If those are needed, the caller passes them in.
- **Independent usefulness.** Someone working on a non-mail project
  could still use the stone. If the only conceivable user is mailrs
  itself, it's not a stone.
- **Measurable on its own.** A stone has clear hot paths that can be
  benched; its perf claims trace to `cargo bench` output.
- **No lower bound on size.** A 50-LOC focused micro-utility is a
  legitimate stone if the identity is single and crisp. The publish
  cost is small; the clarity value of "this is a stone, not cement"
  outweighs the overhead. Larger than ~500 LOC = probably more than
  one stone in disguise.

### Cement (水泥) — specialized, glue, project-bound

Cement is everything else. It binds the stones into the project. Cement is:

- **Tightly coupled to the project's data model, schema, or config.**
- **Specialized to one product** — there's no generic version, the
  shape is dictated by what the product needs.
- **Glue between two or more stones**, with project-specific decisions
  about how they connect.
- **State, wiring, lifecycle management** — startup, shutdown,
  config-from-env, secrets management.

Cement is **not** a failure mode. A solid building needs cement.
The failure mode is **cement masquerading as stone** (i.e. trying
to publish project-specific code as a generic crate) or **stone
masquerading as cement** (i.e. keeping a generic primitive trapped
inside the project where nobody else can use it).

## The self-check

For any code under review (existing or proposed), ask:

| Question | If "no" → it's cement | If "yes" → it's a stone candidate |
|---|---|---|
| Could a non-mailrs project use this as-is? | Cement. Move on. | ✓ |
| Can I name what it does in one sentence? | Probably cement. | ✓ |
| Is the boundary an RFC / standard / well-known algo? | Could still be stone — check next | ✓ |
| Free of project-specific imports? | If imports include `domain_store` / `event_bus` / mailrs config: cement. | ✓ |
| ≤ ~500 LOC of library code? | >500: probably bundled stones, decompose. <50 is fine for micro-utilities — no lower bound. | ✓ |
| Has hot paths worth benching? | Pure I/O glue: cement | ✓ |

All ✓ = extract as a published crate.

## Dedup self-check

If the same shape appears in more than one place, **redundancy is a
design flaw, not a feature.** Carve it out:

1. Identify the shared shape (sliding-window counter, retry math,
   header parsing, …)
2. Extract to a new stone OR consolidate into an existing one
3. Rewrite callers to use the stone; keep their public API stable via
   re-export shim if necessary
4. The dedup IS the value of the refactor. Don't preserve duplicated
   "tuning" — pick one canonical shape and parameterize where needed

Real dedup pass example (2026-05-23): three independent exponential
backoff implementations (`outbound-queue::retry`, `auth-guard::
lockout_duration`, `server::webhook::store::retry_delay_secs`) →
single `mailrs-backoff` primitive with `Jitter` taxonomy.

## Current stones (25 published as of 2026-05-23)

Each row: one-sentence identity → which RFC/concept defines the
boundary → who calls it inside mailrs.

| Stone | Identity | Boundary | Callers |
|---|---|---|---|
| `mailrs-smtp-proto` | SMTP command parsing + session state machine | RFC 5321 | server |
| `mailrs-smtp-client` | Outbound SMTP w/ MX resolution + STARTTLS | RFC 5321 | outbound-queue |
| `mailrs-imap-proto` | IMAP command/response parse + sequence sets | RFC 9051 | server |
| `mailrs-maildir` | Maildir storage on-disk format | maildir spec | mailbox, server |
| `mailrs-mailbox` | Mailbox metadata + threading | algorithmic | server |
| `mailrs-outbound-queue` | Delivery queue, retry, DKIM-sign, DSN, MTA-STS | composite | server |
| `mailrs-ical` | iCalendar (ICS) parse + invite handling | RFC 5545 | server |
| `mailrs-intelligence` | LLM-backed mail classification | adapter | server |
| `mailrs-clean` | HTML sanitizer for inbound mail | algorithmic | server |
| `mailrs-dmarc` | DMARC policy evaluation + reporting | RFC 7489 | server |
| `mailrs-postmaster` | Postmaster bounce/DSN templating | RFC 3464 | server |
| `mailrs-shield` | SMTP anti-spam: greylist + PTR (DNSBL via re-export) | composite | server |
| `mailrs-jmap` | JMAP protocol shapes | RFC 8620 | server |
| `mailrs-dav` | CalDAV / CardDAV protocol | RFC 4791 | server |
| `mailrs-inbound` | Pluggable inbound pipeline stages | trait | server |
| `mailrs-rate-limit` | Token-bucket rate limit keyed by `&str` | algorithmic | server |
| `mailrs-rfc5322` | Lazy RFC 5322 header parser (8-32× vs mail-parser) | RFC 5322 | server |
| `mailrs-rfc2047` | RFC 2047 encoded-word encode + decode | RFC 2047 | server |
| `mailrs-rfc2231` | RFC 2231 MIME parameter encode + decode | RFC 2231 | server |
| `mailrs-auth-guard` | Per-IP failed-auth tracker with lockout | algorithmic | server |
| `mailrs-srs` | Sender Rewriting Scheme (SPF-aware forwarding) | RFC 6730 | server |
| `mailrs-webhook-signature` | HMAC-SHA256 webhook signing + verification | de-facto | server |
| `mailrs-dnsbl` | RFC 5782 DNS-based blocklist lookup | RFC 5782 | server (via shield) |
| `mailrs-clamav` | ClamAV TCP zINSTREAM client | clamd protocol | server |
| `mailrs-backoff` | Exponential backoff w/ AWS jitter taxonomy | algorithmic | outbound-queue, auth-guard, server |

Every stone has:
- README with quickstart + perf table
- BUDGETS.md with regression budgets
- CHANGELOG with keep-a-changelog format
- `tests/perf_gate.rs` enforcing budgets
- `benches/*.rs` for criterion baselines
- All numbers traceable to `cargo bench` (no fabrication)

## Direction: aggressively find more stones

The project's working philosophy (direction 2026-05-23):

> 我们就是要尽量找石头，或者用石头换掉水泥
> 石头和水泥要泾渭分明

Translated: **find as many stones as possible; replace cement with
stones; keep the boundary between stones and cement crystal-clear.**

This means: when in doubt, lean toward "extract." A focused 50-LOC
micro-utility with a crisp single-concept identity is a stone, not
cement. Cement is only what genuinely cannot be lifted out — the
project's specific data model, schema, business rules, wiring.

Reinforcing rule: **diminishing-returns reasoning is wrong.** Even
the 10th, 20th stone is worth finding if it cleans up a boundary.

## Current cement (audited, kept inside the server binary)

The following modules are presently classified cement because they
satisfy at least one of: project-schema-coupled, business-rule-
specific, glue-only-wiring, or session-state-machine. **Re-audit each
periodically — a piece of cement may have a stone hiding inside.**

| Module | LOC | Why it stays |
|---|---|---|
| `web/` routes | ~6000 | Axum handlers wiring stones to HTTP/JSON; mailrs URL paths + auth flow |
| `domain_store.rs` | 2018 | PG-backed domain/account/alias resolver tied to mailrs schema |
| `config.rs` | 1925 | All `MAILRS_*` env vars + validation; pure project config |
| `imap_session.rs` | 2725 | IMAP **session** handler (state machine driving mailrs-imap-proto stone) |
| `smtp_session/` | ~2000 | Same for SMTP; calls inbound pipeline stones |
| `pop3_session.rs` | 622 | POP3 session handler |
| `managesieve_session.rs` | 518 | ManageSieve session handler |
| `sieve.rs` | 1505 | Thin wrapper around upstream `sieve::` crate, translating its events to mailrs delivery actions |
| `mcp/` | ~2800 | Model Context Protocol tools exposing mailrs APIs to LLMs |
| `users.rs` | 729 | Argon2 password backend + LDAP fallback, tied to PG `accounts` |
| `permission.rs` | 345 | Mailrs-specific RBAC permission list + group resolution |
| `api_key_store.rs` | 280 | API key hash + lookup, PG-backed |
| `oidc_store.rs` | 397 | OIDC token storage |
| `web/oidc_provider.rs` | 811 | mailrs's specific OIDC issuer flow |
| `acme.rs` | 310 | Thin glue over `instant-acme` crate + mailrs's challenge token storage |
| `tls.rs` | 50 | arc-swap pattern for hot-reloadable certs |
| `event_bus.rs` | 185 | Wraps `tokio::broadcast` with mailrs's `SmtpEvent` enum |
| `inline_image.rs` | 496 | CID inline image upload/serve tied to maildir |
| `render_preview.rs` | 527 | Chromium-backed mail preview render, tied to mailrs's preview cache |
| `calendar/` | 1547 | Per-account calendar feed using mailrs-ical for parsing |
| `inbound/content_scan.rs` | 638 | mailrs-specific spam scoring rules (ClamAV is now mailrs-clamav stone) |
| `inbound/pipeline.rs` | 909 | Wiring of mailrs-inbound stages with project config |
| `web/conversations.rs` | 1924 | Chat-like conversation view API |
| `dmarc_report.rs` | ~200 | Bridge between mailrs-dmarc + PG report storage |

Total cement ≈ 28k LOC. Total stones (published) ≈ 25 crates.

If you spot something here that satisfies the **all ✓** lens above,
re-audit. If you find a *new* repetition of the same shape across
two or three cement files, you've found a dedup candidate.

## How to use this document

1. **Before adding a new module** — apply the lens. Stone or cement?
2. **Before publishing a new crate** — does it satisfy all ✓?
3. **When you find duplication** — refactor decisively into a stone.
4. **When tempted to expand a stone's API** — does the addition keep
   the single-sentence identity? If not, it's a new stone.
5. **When tempted to put generic code inside cement** — stop, lift it
   into a stone first.

This document is the contract. Update it when stones are added,
identities refined, or cement gets re-evaluated.
