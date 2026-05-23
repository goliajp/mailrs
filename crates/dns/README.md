# mailrs-dns

[![Crates.io](https://img.shields.io/crates/v/mailrs-dns?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-dns)
[![docs.rs](https://img.shields.io/docsrs/mailrs-dns?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-dns)
[![License](https://img.shields.io/crates/l/mailrs-dns?style=flat-square)](#license)

Light `hickory-resolver` wrapper exposing the **5 DNS query types
email servers actually use**: TXT, A, AAAA, MX, PTR. NXDOMAIN is
mapped to `Ok(Vec::new())` consistently across implementors so
caller code doesn't need to special-case it.

The crate exists because mailrs has 4 places that needed nearly the
same resolver shape (mailrs-spf's SpfResolver, mailrs-dkim's
DkimResolver, mailrs-dnsbl's raw use, outbound-queue's MX lookup).
This crate is the unified primitive future versions of those crates
can adopt — without forcing the upgrade today.

## Quickstart

```rust,ignore
use mailrs_dns::{DnsResolver, HickoryResolver};
use hickory_resolver::TokioResolver;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let inner = TokioResolver::builder_tokio()?.build();
let resolver = HickoryResolver::new(inner);

let txts = resolver.lookup_txt("example.com").await?;
let mxs = resolver.lookup_mx("example.com").await?;
# Ok(())
# }
```

## What this crate does

- `DnsResolver` trait with 5 async methods (`lookup_txt`,
  `lookup_a`, `lookup_aaaa`, `lookup_mx`, `lookup_ptr`)
- `HickoryResolver` adapter behind the default `hickory` feature
- `DnsError::Temp` / `DnsError::Perm` distinction (RFC-style
  temperror vs permerror semantics)
- NXDOMAIN → `Ok(Vec::new())` consistently
- Zero deps if you skip the `hickory` feature

## What this crate does not

- **No DNSSEC** — hickory does it; this crate doesn't expose it
- **No recursive resolver** — bring a configured `TokioResolver`
- **No cache** — TTL-aware cache could be a 1.1 addition (mailrs-dnsbl
  already has a use-case-specific cache)
- **No SRV / CAA / DNSKEY** — five types only; email-server scope
- **Not a DNS protocol implementation** — hickory-proto does that.
  This is a *facade* sized to the email-server use case.

## License

Apache-2.0 OR MIT.
