# mailrs-postmaster

[![Crates.io](https://img.shields.io/crates/v/mailrs-postmaster?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-postmaster)
[![docs.rs](https://img.shields.io/docsrs/mailrs-postmaster?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-postmaster)
[![License](https://img.shields.io/crates/l/mailrs-postmaster?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-postmaster?style=flat-square)](https://crates.io/crates/mailrs-postmaster)

One-shot email-domain DNS health check for mail-server operators — MX, SPF, DKIM, DMARC, MTA-STS, TLS-RPT, BIMI, DANE, PTR/FCrDNS, all in one call.

Extracted from [mailrs] so any Rust project running a mail server (or a CLI tool, monitoring agent, or admin UI) can answer "is this domain configured correctly?" without re-implementing 9 different DNS RFCs.

## Highlights

- **Single entry point** — [`check_domain(resolver, domain, dkim_selector, hostname)`] returns one [`DomainCheckReport`] with all 10 check results.
- **9 RFCs covered** —
  RFC 5321 (MX) ·
  RFC 7208 (SPF) ·
  RFC 6376 (DKIM) ·
  RFC 7489 (DMARC) ·
  RFC 8461 (MTA-STS) ·
  RFC 8460 (TLS-RPT) ·
  BIMI draft ·
  RFC 7671 (DANE) ·
  RFC 1912 (PTR/FCrDNS).
- **Structured output** — every check has a [`Status`] (`Pass` / `Warn` / `Fail` / `Skip`), human message, and per-check details vec. The report is `Serialize` so it ships straight to JSON / Prometheus / a CLI table.
- **Minimal deps** — `hickory-resolver` for DNS, `reqwest` for the one HTTPS fetch the MTA-STS policy needs.

## Quick start

```rust,no_run
use hickory_resolver::TokioResolver;
use mailrs_postmaster::check_domain;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = TokioResolver::builder_tokio()?.build()?;
let report = check_domain(&resolver, "example.com", Some("default"), "mail.example.com").await;

for check in &report.checks {
    println!("{}: {:?} — {}", check.name, check.status, check.message);
    for detail in &check.details {
        println!("    {}", detail);
    }
}
# Ok(())
# }
```

Output (example):

```text
MX Records: Pass — 1 MX record found
SPF: Pass — SPF record found and aligned
DKIM: Warn — selector "default" returns no record
DMARC: Pass — policy=quarantine, rua present
MTA-STS Record: Pass — id=20260101
MTA-STS Policy: Pass — mode=enforce, mx=mail.example.com
TLS-RPT: Pass — rua=mailto:tlsrpt@example.com
PTR: Pass — FCrDNS matches mail.example.com
DANE: Skip — no TLSA records (DANE optional for SMTP)
BIMI: Pass — selector default, logo URL valid
```

## What's covered per check

| Check | What we look up | Pass when |
|---|---|---|
| MX | `MX domain` | at least one record, sorted by priority |
| SPF | `TXT domain` for `v=spf1` | record present + alignment with MX |
| DKIM | `TXT <selector>._domainkey.domain` | key present and well-formed |
| DMARC | `TXT _dmarc.domain` | `v=DMARC1; p=...` + rua |
| MTA-STS Record | `TXT _mta-sts.domain` | `v=STSv1; id=...` |
| MTA-STS Policy | HTTPS `mta-sts.domain/.well-known/mta-sts.txt` | parseable, mode set, mx list present |
| TLS-RPT | `TXT _smtp._tls.domain` | `v=TLSRPTv1; rua=...` |
| BIMI | `TXT default._bimi.domain` | record present + reachable logo URL |
| DANE | `TLSA _25._tcp.mx-host` | at least one record per MX host |
| PTR | reverse of `hostname` IP, then forward | reverse → forward roundtrip matches |

## Why a single `check_domain` call

Mail-server tooling repeatedly asks the same question: "is `foo.com` set up correctly?" Splitting the answer across 9 separate function calls forces every consumer to re-glue them — and they always end up writing the same retry, ordering, and result-shape code. One call → one report keeps the friction off the consumer.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
