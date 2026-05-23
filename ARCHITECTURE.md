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

## Current stones (33 published as of 2026-05-23)

Each row: one-sentence identity → which RFC/concept defines the
boundary → who calls it inside mailrs.

### Protocol parsers (zero-I/O, used on every connection)

| Stone | Identity | Boundary | Callers |
|---|---|---|---|
| `mailrs-smtp-proto` | SMTP command parsing + session state machine (beats `smtp-codec` 2-7×) | RFC 5321 | server |
| `mailrs-imap-proto` | IMAP command/response parse + sequence sets (beats `imap-codec` on FETCH 2.2×) | RFC 9051 | server |
| `mailrs-rfc5322` | Lazy RFC 5322 header parser (10-33× vs `mail-parser`, **31× vs Go `net/mail`**) | RFC 5322 | server, mime |
| `mailrs-rfc2047` | RFC 2047 encoded-word encode + decode (4-14× vs `mail-parser`) | RFC 2047 | server, mime |
| `mailrs-rfc2231` | RFC 2231 MIME parameter encode + decode | RFC 2231 | server, mime |
| `mailrs-mime` | RFC 2045/2046 MIME body tree parser | RFC 2045/6 | server |
| `mailrs-ical` | iCalendar (ICS) parse + invite handling (3.4-3.7× vs `icalendar`, **4× vs C `libical`**) | RFC 5545 | server |
| `mailrs-jmap` | JMAP protocol shapes (RFC 8620 + 8621) | RFC 8620 | server |
| `mailrs-dav` | CalDAV / CardDAV protocol handlers | RFC 4791 / 6352 | server |

### Email-auth (**DEPS_AUDIT #1 — `mail-auth` fully replaced**)

| Stone | Identity | Boundary | Callers |
|---|---|---|---|
| `mailrs-spf` | RFC 7208 SPF record parser + verifier (beats mail-auth on realistic + pathological inputs) | RFC 7208 | server |
| `mailrs-dkim` | RFC 6376 DKIM signer + verifier (beats mail-auth on both inputs since 1.1.3) | RFC 6376 | server |
| `mailrs-dmarc` | DMARC TXT policy parse + alignment (DKIM `d=` / SPF MAIL FROM, strict + relaxed via Public Suffix List) + pure-fn `evaluate` + aggregate XML reporting | RFC 7489 | server |
| `mailrs-arc` | Authenticated Received Chain — 3 header parsers + chain extract + structural verify. Crypto AMS/AS verify reserved for 1.1, reuses `mailrs-dkim` canon. Only standalone Rust ARC implementation (mail-auth bundles it). | RFC 8617 | server (1.1) |
| `mailrs-srs` | Sender Rewriting Scheme (SPF-aware forwarding) | RFC 6730 | server |

### Infra / utilities

| Stone | Identity | Boundary | Callers |
|---|---|---|---|
| `mailrs-rate-limit` | Token-bucket rate limit keyed by `&str` (matches/beats `governor` on hot path since 1.0.3) | algorithmic | server |
| `mailrs-auth-guard` | Per-IP failed-auth tracker with lockout | algorithmic | server |
| `mailrs-dnsbl` | DNS-based blocklist lookup with TTL cache | RFC 5782 | server (via shield) |
| `mailrs-clamav` | ClamAV TCP INSTREAM client | clamd protocol | server |
| `mailrs-backoff` | Exponential backoff w/ AWS jitter taxonomy (8-26× vs `exponential-backoff`) | algorithmic | outbound-queue, auth-guard, server |
| `mailrs-webhook-signature` | HMAC-SHA256 webhook signing + verification w/ secret rotation | de-facto | server |
| `mailrs-tls-reload` | Hot-reloadable rustls `ServerConfig` via arc-swap + PEM loader | rustls integration | server, acme |
| `mailrs-acme` | ACME (RFC 8555 / Let's Encrypt) orchestration + HTTP-01 + renewal | RFC 8555 | server |
| `mailrs-dns` | Thin hickory-resolver wrapper exposing only TXT / A / AAAA / MX / PTR | hickory + uniform shape | (future: spf/dkim/dnsbl migration target) |
| `mailrs-mta-sts` | RFC 8461 STS record + policy parser, MX wildcard match, `enforce(&Policy, mx)` decision, Cache trait (no HTTP / DNS in-crate) | RFC 8461 | server (outbound-queue MTA-STS path) |

### Server building blocks (opinionated, but BYO-store)

| Stone | Identity | Boundary | Callers |
|---|---|---|---|
| `mailrs-smtp-client` | Outbound SMTP w/ MX resolution + STARTTLS | RFC 5321 client | outbound-queue |
| `mailrs-maildir` | Maildir storage on-disk format | maildir spec | mailbox, server |
| `mailrs-mailbox` | Mailbox metadata + threading (sqlx + maildir) | algorithmic | server |
| `mailrs-outbound-queue` | Delivery queue, retry, DKIM-sign, DSN, MTA-STS | composite | server |
| `mailrs-inbound` | Pluggable inbound pipeline stages + Authentication-Results helpers (RFC 8601) | trait | server |
| `mailrs-shield` | SMTP anti-spam: greylist + PTR / FCrDNS (DNSBL via re-export) | composite | server |
| `mailrs-postmaster` | Postmaster DNS health checks (MX/SPF/DKIM/DMARC/MTA-STS/TLS-RPT/BIMI/DANE/PTR) | RFC 3464 + diagnostics | server |
| `mailrs-intelligence` | LLM-backed mail classification + structured extraction + embeddings | adapter | server |
| `mailrs-clean` | HTML sanitizer + tracking-pixel detection + quoted-reply splitter | algorithmic | server |

### Standard practices on every stone

- README with quickstart + perf table + competitor comparison (when one exists)
- BUDGETS.md with regression budgets (`tests/perf_gate.rs` enforces in CI)
- CHANGELOG.md in keep-a-changelog format
- `benches/*.rs` for criterion baselines, including `compare_<competitor>.rs` for the 8 head-to-head cases
- `fuzz/` libFuzzer targets on every untrusted-input parser (8 crates, 13 targets, **~120M iter run, 1 real bug caught + fixed**)
- `#![deny(missing_docs)]` gate
- `#[deny(warnings)]` + `#[deny(clippy::all)]` workspace-wide
- All perf numbers traceable to `cargo bench` output (no fabrication — see [PERFORMANCE.md](./PERFORMANCE.md) ledger)

### Cross-language posture

`bench-harness/` runs the same workloads against C / Go competitors:

| Scenario | mailrs | competitor | gap |
|---|---:|---:|---|
| RFC 5322 read + Subject + From | 46 ns | Go `net/mail`: 1440 ns | **mailrs 31×** |
| iCalendar parse | 1.76 µs | C `libical` 4.0: 7.03 µs | **mailrs 4×** |
| SMTP EHLO parse | 18 ns | Rust `smtp-codec`: 126 ns | **mailrs 7×** |
| DKIM-Signature minimal parse | 147 ns | Rust `mail-auth` 0.9: 167 ns | **mailrs +12%** |
| ... | | | (full table in PERFORMANCE.md) |

## Direction: aggressively find more stones

The project's founding direction (2026-05-23):

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

### Where the cycle stands (end of 2026-05-23)

The aggressive stone-finding cycle from 7 → 31 published crates is
**complete for the obvious shapes**:

- All email protocols (SMTP, IMAP, JMAP, CalDAV/CardDAV, ManageSieve
  via upstream `sieve`)
- All RFC primitives we hit on the hot path (5322, 2045/6, 2047, 2231,
  5545, 5782, 5321, 9051, 7208, 6376, 7489, 8617 still to come)
- All identifiable infra primitives (rate limit, backoff, auth-guard,
  webhook-signature, tls-reload, acme)
- DEPS_AUDIT #1 — `mail-auth` — fully replaced (SPF + DKIM + DMARC)

What's still open:

1. **ARC (RFC 8617)** — Authenticated Received Chain, the
   forwarding-friendly successor to DKIM/SPF/DMARC. Natural sibling
   to the email-auth trio; planned `mailrs-arc` 1.0.
2. **MTA-STS (RFC 8461)** — currently lives inside `mailrs-postmaster`
   as a health-check; could carve out `mailrs-mta-sts` doing real
   policy lookup + cache + decide.
3. **Sieve eval** — `sieve-rs` is still an external dep; rewrite is a
   large lift (~2000 LOC of RFC 5228 + extensions), deferred.

After those land, the natural next axis is **server-level polish**:
end-to-end SMTP/IMAP throughput, observability, deploy story.
Cement that resists further extraction is genuinely cement.

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
| `acme.rs` | 310 | Thin glue over `mailrs-acme` stone + mailrs's challenge token storage (`mailrs-acme` 1.0 has the orchestration; this file is the project-specific store binding) |
| `tls.rs` | 50 | Thin glue over `mailrs-tls-reload` stone for hot-reloadable certs |
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
