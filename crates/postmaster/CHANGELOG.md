# Changelog

All notable changes to `mailrs-postmaster` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-08-04

### Added

- `PostmasterResolver` trait (TXT, MX, A/AAAA, PTR and TLSA lookups) with
  `MxRecord` and `ResolverError`, so the checks can run on any DNS
  backend.
- `HickoryPostmasterResolver`, wrapping a `hickory_resolver::TokioResolver`
  (feature `hickory`, on by default). Without the feature the crate has
  no hickory dependency.
- `MockResolver` with `with_txt` / `with_mx` / `with_ip` / `with_reverse` /
  `with_tlsa` (and `*_err`) builders, for testing without a network.
- Pure helpers exported: `parse_mta_sts_policy`, `validate_tlsrpt_record`,
  `extract_tlsrpt_rua`.

### Changed

- **Breaking:** `check_domain` and `lookup_bimi_logo` take
  `&impl PostmasterResolver` instead of `&TokioResolver`. Wrap an
  existing resolver with `HickoryPostmasterResolver::new(resolver)`.
  This change shipped in a minor release.

## [1.0.1] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.0] - 2026-05-20

### Added
- Initial release. Async email-domain DNS health checks: MX, SPF, DKIM, DMARC, MTA-STS, TLS-RPT, BIMI, DANE, and PTR. Zero I/O outside the resolver.
