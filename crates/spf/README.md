# mailrs-spf

[![Crates.io](https://img.shields.io/crates/v/mailrs-spf?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-spf)
[![docs.rs](https://img.shields.io/docsrs/mailrs-spf?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-spf)
[![License](https://img.shields.io/crates/l/mailrs-spf?style=flat-square)](#license)

RFC 7208 Sender Policy Framework verifier. Pure-Rust evaluator with a
pluggable DNS resolver trait; ships an optional
`hickory-resolver`-backed implementation behind the `hickory` feature.

Pairs with [`mailrs-rfc5322`](https://crates.io/crates/mailrs-rfc5322)
and [`mailrs-dmarc`](https://crates.io/crates/mailrs-dmarc) to give
mailrs a full owned email-auth stack — replacing the SPF half of
`mail-auth` with shape we control.

## Quickstart

```rust,ignore
use mailrs_spf::{verify, VerifyInput, SpfResult, HickoryResolver};
use hickory_resolver::TokioResolver;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver_inner = TokioResolver::builder_tokio()?.build();
let resolver = HickoryResolver::new(resolver_inner);

let input = VerifyInput {
    ip: "203.0.113.42".parse()?,
    helo: "mta.example.com".into(),
    mail_from: "alice@example.com".into(),
};

let result = verify(&resolver, &input).await;
match result {
    SpfResult::Pass => { /* accept */ }
    SpfResult::Fail => { /* reject 5xx with SPF reason */ }
    SpfResult::SoftFail => { /* accept but tag suspicious */ }
    SpfResult::Neutral | SpfResult::None => { /* no policy / no record */ }
    SpfResult::PermError | SpfResult::TempError => { /* see RFC 7208 §8 */ }
}
# Ok(())
# }
```

## What this crate does

- Parse SPF TXT records into a typed [`Record`] with [`Mechanism`]s
- Evaluate against `(IP, HELO, MAIL FROM)` per RFC 7208 §4
- All seven result values: `none / pass / fail / softfail / neutral
  / permerror / temperror`
- Mechanism support: `all`, `ip4`, `ip6`, `a`, `mx`, `include`,
  `exists`
- Qualifier support: `+` (default), `-`, `~`, `?`
- DNS lookup budget (≤10 per RFC §4.6.4) + recursion depth cap
- Multi-record detection (multiple `v=spf1` → `PermError` per §4.5)
- DNS resolver trait so callers plug their own DNS (hickory included
  behind a feature flag)

## What this crate does not (yet)

These are out-of-scope for 1.0 and deferred to 1.x minors:

- **Macro expansion** (RFC 7208 §7) — `%{i}`, `%{s}`, `%{d}`, etc. in
  `exists:` / `include:` domain templates. Common patterns work
  because most SPF records use literal domains; macro-heavy records
  (some bulk-mailer providers use `exists:%{ir}._spf.provider.com`)
  will compute against the literal template string. Add `macros`
  feature when expansion is needed.
- **`redirect=` modifier** (RFC 7208 §6.1) — would extend the lookup
  to another domain. Detected and skipped without erroring.
- **`exp=` modifier** (§6.2) — explanation text on Fail. Detected
  and skipped.
- **`ptr` mechanism** (§5.5) — RFC marks it not-recommended; we
  return `PermError` if a record uses it.

These are intentional v1 scope limits, not bugs. None of them affect
the common case (literal-domain records from major senders); add as
1.x minors when a use case demands.

## Why a new crate?

`mail-auth` covers SPF + DKIM + DMARC + ARC in one crate. We use it
in mailrs's inbound pipeline. The shape works but:

- The combined surface is heavy for the SPF use case alone
- We can't measure its perf cleanly against `mailrs-rfc5322` (the
  underlying message-parsing layer)
- Owning SPF + DKIM + DMARC as separate, focused stones lets us
  tune each per the dep-audit doc

`mailrs-dmarc` already exists. This crate carves the SPF half;
`mailrs-dkim` will follow.

## Performance

Measured (criterion, M-series Mac, release):

| Operation | Median |
|---|---:|
| `Record::parse` (simple `v=spf1 ip4 -all`) | **63 ns** |
| `Record::parse` (complex 8-mechanism record) | **360 ns** |
| `Record::parse` (8-include pathological) | **400 ns** |
| `verify` pass-path (no real DNS) | **244 ns** |

### Vs. `mail-auth` 0.9 (the de-facto Rust competitor)

| Input | mailrs-spf 1.0.4 | mail-auth 0.9 | Winner |
|---|---:|---:|---|
| `v=spf1 ip4:… -all` (3 mech) | 63 ns | 50 ns | mail-auth +25% ⚠ |
| 8-mechanism complex | **360 ns** | 410 ns | **mailrs +14%** ✅ |
| 8-include pathological | **400 ns** | 577 ns | **mailrs +44%** ✅ |

mailrs wins anything realistic-sized; mail-auth holds a 13 ns edge
on tiny 3-mechanism records because their hand-rolled byte-iter IPv4
parser is tighter than `std::net::Ipv4Addr::FromStr`. Reproduce via
`cargo bench -p mailrs-spf --bench compare_mail_auth`.

The verify number is the pure CPU work; actual production `verify`
is dominated by DNS round-trips (typical 5-50 ms). Reproduce:
`cargo bench -p mailrs-spf --bench spf`.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-spf`) |
| **test** | line cov: 90.5% (`cargo llvm-cov -p mailrs-spf --summary-only`) |
| **bench** | ✅ 2 file(s) criterion + ✅ 2 gate(s) `perf_gate.rs` |
| **size** | release rlib: 2.3 MB |
| **fuzz** | ✅ 1 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons (from PERFORMANCE.md)

- `mailrs-spf` vs `mail-auth` 0.9 (SPF half — the DEPS_AUDIT #1 reason)

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
