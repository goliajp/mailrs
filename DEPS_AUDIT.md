# External Dependency Audit — Status Ledger

> "Let's run a dependency review and see which ones we could reasonably
> reimplement in-house."
> — project direction, 2026-05-23

This document asks of each external dependency: *is there a clean,
RFC-bounded reimplementation hiding inside it* — one focused enough that
we could own the performance / API / shape end-to-end as an in-house
library crate?

**Status (end of 2026-05-23): the four rewrite candidates identified in
the original audit are all resolved.** What follows is the audited
state, not a fresh plan.

## ✅ Candidates resolved

### #1 — `mail-auth` (SPF + DKIM + DMARC + ARC) → **fully replaced**

Owned by Stalwart, used by mailrs for SPF / DKIM / DMARC verification.

Resolution: three in-house crates, each beating `mail-auth` on the
realistic inputs we measured against it:

| Crate | Version | Vs. mail-auth |
|---|---|---|
| `mailrs-spf` | 1.0.4 | wins complex (+14%) and pathological (+44%); loses simple by 13 ns (std `Ipv4Addr` cost) |
| `mailrs-dkim` | 1.1.3 | wins both minimal (+12%) and realistic (+4%) since the byte-match dispatch + `h=` byte-iter rewrite |
| `mailrs-dmarc` | 1.1.0 | RFC 7489 §6 policy eval + alignment (strict + relaxed via PSL) + aggregate reporting all owned |
| `mailrs-arc` | 1.0.0 | RFC 8617 ARC headers + chain extract + structural verify (cv= integrity, contiguity). Cryptographic AMS+AS verify reserved for 1.1 — that completes the `mail-auth` runtime-drop. |

**Status:** SPF, DKIM, DMARC are full replacements. ARC 1.0 covers
structural verification — enough to reject malformed chains before any
DNS work. `mail-auth` becomes fully removable from the server once
**mailrs-arc 1.1** ships the cryptographic AMS + AS verification layer
(reuses `mailrs-dkim` canon for byte-identical canonicalization).

### #2 — `mail-parser` → **replaced for the email-auth + lookup paths**

| What we replaced | New crate | Win |
|---|---|---|
| Header lookup (Subject, From, Received, …) | `mailrs-rfc5322` 1.0.1 | 10-33× vs mail-parser on the same op |
| Encoded-word decode (Subject / display name) | `mailrs-rfc2047` 1.1.2 | 4-14× vs mail-parser for single-field extraction |
| Filename parameter decode (Content-Disposition) | `mailrs-rfc2231` 1.0.0 | filled a gap mail-parser doesn't address standalone |
| MIME body tree (multipart, attachments) | `mailrs-mime` 1.0.3 | wins simple `body_text` +17%; loses simple parse path ~60% (mail-parser has years of MIME-specific optimization we'd need to match) |

**`mail-parser` is no longer used on the inbound hot path.** Residual
use in the server (per `cargo tree`) is only via the `mail-auth` chain,
which is itself slated for removal once ARC lands.

### #3 — `hickory-resolver` → **wrapper crate shipped (`mailrs-dns` 1.0)**

`hickory-resolver` is the right base; we shipped a thin wrapper
exposing only the 5 query types email servers actually use (TXT, A,
AAAA, MX, PTR) with a uniform `Result<Vec<_>, DnsError>` shape
(NXDOMAIN → `Ok(Vec::new())`).

`mailrs-dns` 1.0.0 is published; future minor versions of `mailrs-spf`
/ `mailrs-dkim` / `mailrs-dnsbl` will migrate to it as their resolver
trait. **Not blocking — each currently has its own minimal resolver
trait that does the job.**

### #4 — `sieve-rs` → **stays (large scope, lower ROI)**

RFC 5228 Sieve eval is ~2000+ LOC of bounded but intricate spec work.
The upstream crate is functional. Defer indefinitely; revisit only
if mailrs's Sieve usage outgrows the upstream shape.

## Don't rewrite — foundational or already optimal

These deps are the foundation our own crates build on. Reimplementing
would be wasted effort or a security regression.

| Dep | Why it stays |
|---|---|
| `tokio` / `tokio-util` / `tokio-rustls` / `async-trait` / `futures-util` | The async runtime; no contest |
| `serde` / `serde_json` / `thiserror` / `toml` | Foundational; no replaceable shape |
| `tracing` / `tracing-subscriber` | Standard log/trace layer |
| `chrono` / `chrono-tz` / `uuid` | Time + ID primitives |
| `bytes` / `urlencoding` / `data-encoding` / `flate2` / `tempfile` | Byte / encoding / compression / fs primitives |
| `hmac` / `sha2` / `argon2` / `rsa` / `jsonwebtoken` / `password-hash` | **Crypto** — never roll your own. Already best-of-breed. |
| `rustls` / `rustls-pki-types` / `webpki-roots` / `x509-parser` | TLS / X.509 — same; rustls owns the Rust TLS ecosystem |
| `rcgen` | Cert generation; only used in tests |
| `arc-swap` | Lock-free shared pointer; primitive |
| `dashmap` | Sharded HashMap; primitive (we already use it heavily) |
| `axum` / `tower-http` / `http` / `reqwest` | Web stack; replacing would be a project the size of this one |
| `sqlx` / `redis` | DB clients; mature and well-shaped |
| `criterion` | Bench framework; standard |
| `ldap3` | LDAP fallback auth; specialized |
| `chromiumoxide` | Chromium control for `render_preview`; specialized |
| `instant-acme` | Low-level ACME protocol primitives; we wrap with `mailrs-acme` 1.0 |
| `psl` | Compile-time Public Suffix List; used by `mailrs-dmarc` for org-domain extraction |
| `quanta` | Fast monotonic clock (mach_absolute_time / TSC); adopted by `mailrs-rate-limit` 1.0.3 from `governor`'s own playbook |
| `image` / `pdf-extract` / `html2text` | Specialized content extractors |
| `rmcp` | MCP protocol; specialized |
| `rrule` | iCal recurrence rules; specialized RFC primitive (used by `mailrs-ical`) |
| `totp-rs` | TOTP; specialized RFC primitive |
| `encoding_rs` | WHATWG charset table; foundational for rfc2047/2231/mime |
| `base64` / `hex` | Encoding primitives |
| `hostname` / `filetime` / `schemars` | Tiny utility crates |
| `rand_core` | Foundational |

## Possible-but-deferred (re-audit periodically)

| Dep | Why we'd consider it | Why we're not, yet |
|---|---|---|
| `sieve-rs` | Inbound filtering, ~2000 LOC of RFC 5228 + extensions | Functional; scope is high and demand inside mailrs is steady-state |
| `mail-builder` | Outbound MIME builder for DSN / report mails | Used in low-traffic paths (DSN, DMARC aggregate); not hot enough to justify |
| `lettre` | We don't use it (we have `mailrs-smtp-client`) | No-op for us; listed only to mark "we considered it" |

## Next round of in-house crates (planned)

These don't replace existing deps — they fill gaps in the Rust email
ecosystem we'd benefit from owning end-to-end:

| Planned crate | Boundary | Why | Status |
|---|---|---|---|
| `mailrs-arc` 1.0 | RFC 8617 — Authenticated Received Chain (DKIM/SPF/DMARC chained across forwarders) | Closes the email-auth quartet; reuses our `mailrs-dkim` canon + verify | ✅ Shipped — structural verify in 1.0, **crypto AMS/AS verify in 1.1** (RSA-SHA256 + Ed25519-SHA256, end-to-end roundtrip-tested with real RSA-2048 keypairs) |
| `mailrs-mta-sts` 1.0 | RFC 8461 — MTA Strict Transport Security policy lookup + cache + decide | Currently embedded in `mailrs-postmaster` as a diagnostic only; lift to a real policy enforcer | ✅ Shipped 2026-05-23 — parsers + `enforce(&Policy, mx)` + Cache trait + `InMemoryCache` |

## Self-check (template for the next audit pass)

Apply the "is this rewrite-worth-it?" filter to each candidate:

- [ ] Is the upstream dep doing something well-defined by an RFC?
- [ ] Do we use it on a path where perf / shape matters?
- [ ] Is the scope < 2000 LOC of bounded code (no open-ended scope creep)?
- [ ] Can we measure the upstream's perf so the "did we improve" claim
      is honest, not vibes?
- [ ] Does it line up with our existing crate family (so server
      adoption is clean, not a sideways port)?

All ✅ → reimplement it in-house. The original four candidates all
passed; the next two (`mailrs-arc`, `mailrs-mta-sts`) also pass, and
both are now shipped.
