# External Dependency Audit — Rewrite Candidates

> "我们要做一轮 deps 审查，看有没有哪些有重写的可能"  
> — direction, 2026-05-23

Pass applies the **stone / cement lens** outward: each external crate
we depend on is asked: *is there a clean mailrs stone hiding inside
this dep?* — i.e. a focused, RFC-bounded reimplementation that would
let us own the perf / API / shape end-to-end.

## Classification

### Don't rewrite — foundational or already optimal

These deps are the stones underneath our stones. Reimplementing would
be wasted effort or a security regression.

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
| `instant-acme` | Low-level ACME protocol; we wrap with `mailrs-acme`. Good shape. |
| `image` / `pdf-extract` / `html2text` | Specialized content extractors |
| `rmcp` | MCP protocol; specialized |
| `rrule` | iCal recurrence rules; specialized RFC primitive |
| `totp-rs` | TOTP; specialized RFC primitive |
| `hostname` / `filetime` / `schemars` | Tiny utility crates |
| `rand_core` / `tempfile` | Foundational |

### Candidates to rewrite — high ROI

These are the deps where mailrs has BOTH (a) a clear use case and
(b) the dep is heavy / general-purpose / not perf-tuned for our path.

| Rank | Dep | Why rewrite? | Proposed stone(s) | Effort |
|---:|---|---|---|---|
| **1** | `mail-auth` (SPF + DKIM + DMARC + ARC) | We use it on the inbound hot path for every received message. Heavy crate; perf unknown to us; opaque integration with our `mailrs-rfc5322` (which is 8-32× faster than mail-parser). We already have `mailrs-dmarc` — owning SPF + DKIM rounds out the email auth suite. | **`mailrs-spf`** (RFC 7208), **`mailrs-dkim`** (RFC 6376) | SPF ~600 LOC, DKIM ~1000 LOC — substantial but bounded |
| **2** | `mail-parser` (residual) | Already replaced for header lookup (`mailrs-rfc5322`). Still used for MIME body + multipart + attachment extraction in 4-5 server modules. Building our own MIME body parser gives us full control of the inbound parse stack. | **`mailrs-mime`** (RFC 2045/2046 MIME body) | ~1500-2000 LOC; bigger lift |
| **3** | `hickory-resolver` | Full DNS resolver. We need MX, A/AAAA, TXT, PTR. hickory has DNSSEC + recursive + many features we don't use. | **`mailrs-dns`** (thin wrapper over `hickory-proto` for the 4 query types) | ~300-500 LOC; medium |
| **4** | `sieve-rs` | Sieve script eval (RFC 5228). Used by mailrs's filtering rules. Large scope to rewrite. | **`mailrs-sieve`** | ~2000+ LOC; defer |

### Already replaced (history)

| Old dep usage | New stone | Where it was carved |
|---|---|---|
| `mail-parser` for header lookup | `mailrs-rfc5322` | Hot inbound path, 8-32× faster |
| `mail-parser` for encoded-words | `mailrs-rfc2047` | Subject/From decode |
| `mail-parser` for filename params | `mailrs-rfc2231` | Content-Disposition decode |

These ARE legitimate "we replaced an existing dep usage with our own
stone" wins. The remaining `mail-parser` usage is for the parts our
stones don't yet cover (MIME body tree).

## Action plan

Start with the **highest ROI: `mailrs-spf` 1.0.0**.

Rationale:
- SPF is a single, bounded RFC (7208).
- Used per-inbound-message → real hot path.
- Pairs naturally with `mailrs-rfc5322` + `mailrs-dmarc` (we own the
  envelope + header + DMARC ends; SPF closes the gap).
- ~600 LOC scope is manageable in one autorun batch.
- We can compare measured perf against `mail-auth`.

After SPF:
- `mailrs-dkim` (next sibling).
- Then re-audit: does `mail-auth` still earn its keep, or can the
  server drop it entirely?

`mailrs-mime` is high value but a bigger commitment — defer until
after the email-auth trio is owned.

## Self-check

Apply the "is this rewrite-worth-it?" filter to each candidate:

- [ ] Is the upstream dep doing something well-defined by an RFC?
- [ ] Do we use it on a path where perf / shape matters?
- [ ] Is the scope < 2000 LOC of bounded code (no open-ended scope creep)?
- [ ] Can we measure the upstream's perf so the "did we improve" claim
      is honest, not vibes?
- [ ] Does it line up with our existing stone family (so server
      adoption is clean, not a sideways port)?

`mailrs-spf` passes all five. Proceeding.
