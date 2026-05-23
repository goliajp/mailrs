# Changelog

All notable changes to `mailrs-dns` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-23

### Added

- DEPS_AUDIT #3 stone. Light hickory-resolver wrapper for the
  email-server use case.
- `DnsResolver` trait with 5 async methods (lookup_txt / _a / _aaaa
  / _mx / _ptr).
- `HickoryResolver` adapter behind default `hickory` feature.
- `DnsError::Temp` / `::Perm` distinction.
- NXDOMAIN consistently mapped to `Ok(Vec::new())`.
- 2 inline tests (error display + eq).

### Future adoption (informational, no forced migration)

- `mailrs-spf` 1.1: switch `SpfResolver` to depend on `DnsResolver`
- `mailrs-dkim` 1.2: same for `DkimResolver`
- `mailrs-dnsbl` 1.1: replace raw `TokioResolver` usage
- Server's outbound MX lookup: replace direct hickory call

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-dns-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-dns-v1.0.0
