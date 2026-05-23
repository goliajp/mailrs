# mailrs-dkim

[![Crates.io](https://img.shields.io/crates/v/mailrs-dkim?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-dkim)
[![docs.rs](https://img.shields.io/docsrs/mailrs-dkim?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-dkim)
[![License](https://img.shields.io/crates/l/mailrs-dkim?style=flat-square)](#license)

RFC 6376 DKIM signature verifier. Pairs with
[`mailrs-spf`](https://crates.io/crates/mailrs-spf) and
[`mailrs-rfc5322`](https://crates.io/crates/mailrs-rfc5322) to give
mailrs full ownership of the inbound email-auth stack — together
they replace the `mail-auth` umbrella crate with shape we control.

## Quickstart

```rust,ignore
use mailrs_dkim::{verify, HickoryDkimResolver};
use hickory_resolver::TokioResolver;

# async fn run(raw_message: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let inner = TokioResolver::builder_tokio()?.build();
let resolver = HickoryDkimResolver::new(inner);

let result = verify(&resolver, raw_message).await;
// Returns mailrs_dkim::DkimResult: Pass / Fail / Neutral / TempError /
// PermError / None / Policy.
# Ok(())
# }
```

## What this crate does (1.0)

- Parse `DKIM-Signature:` headers (all tags: v, a, b, bh, c, d, h,
  l, q, s, t, x, i, z).
- Canonicalization: **simple/simple, simple/relaxed, relaxed/simple,
  relaxed/relaxed** — all four combinations per RFC 6376 §3.4.
- Body hash computation (SHA-256) with optional length limit (`l=`).
- Header hash computation over the signed-header list in order,
  with the DKIM-Signature value itself appended with the `b=` tag
  cleared (RFC 6376 §3.7).
- Public-key fetch: TXT at `<selector>._domainkey.<domain>`.
- Signature verification: **`a=rsa-sha256`** (covers ~99% of
  real-world DKIM in 2026).
- Returns the seven RFC 8601 vocabulary values:
  `none / pass / fail / neutral / temperror / permerror / policy`.
- Pluggable [`DkimResolver`] async trait — bring your own DNS or
  use the bundled hickory-backed adapter.

## What this crate does not (yet)

- **`a=ed25519-sha256`** (RFC 8463) — modern but rare. Deferred to
  1.1. The header parser rejects with `UnsupportedAlgorithm`; treat
  this as `PermError`.
- **Multiple signatures**: `verify` finds the **first**
  DKIM-Signature header. Most legitimate mail signs once; some
  ARC-bridged mail signs multiple times. The trait shape allows a
  future `verify_all` without breaking changes.
- **DSN bounce signature verification** — same protocol shape, but
  the DKIM-Signature is on a different header anchor; future addition.
- **Author Domain Signing Practices** — historical (RFC 5617),
  deprecated.

## Why a new crate?

`mail-auth` includes DKIM but bundles it with SPF + DMARC + ARC. By
the project's DEPS_AUDIT we want each in a focused stone so:
- the perf of each step is measurable independently
- the API shape is tunable per-RFC
- the crate's transitive deps stay tight per use case

`mailrs-spf` shipped first (DEPS_AUDIT #1). This is its sibling.

## Performance

Measured (criterion, M-series Mac, release):

| Operation | Median |
|---|---:|
| `DkimHeader::parse` (minimal header) | **700 ns** |
| `DkimHeader::parse` (realistic 7-tag header) | **1.49 µs** |
| `canonicalize_body` (simple) | **70 ns** |
| `canonicalize_body` (relaxed) | **140 ns** |
| `canonicalize_header` (relaxed, one header) | **85 ns** |

Production `verify` is dominated by:
- DNS lookup for the selector key (5-50 ms)
- RSA-SHA256 signature verification (~1-2 ms for 2048-bit keys)

The bench numbers above are the pure CPU pieces. Reproduce:
`cargo bench -p mailrs-dkim --bench dkim`.

## License

Apache-2.0 OR MIT.
