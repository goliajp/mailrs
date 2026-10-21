# Changelog

All notable changes to `mailrs-outbound-queue` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `dkim_signers::usable_selectors(&DkimSignConfig)` — the domains a
  loaded config can actually sign for, with their selectors. Every key is
  parsed up front, so a domain whose key file holds no valid key is left
  out instead of looking configured until its first message fails to
  sign.

### Changed

- `MAILRS_DKIM_KEYS` parsing (`dkim_env::extra_keys_from_env`) and the
  per-domain key lookup in `DkimSignConfig` now use
  `mailrs_dkim::key_list`, so the signer and any report on which key
  signs a domain give the same answer. Requires the `mailrs-dkim` release
  that adds `key_list`. Skipped entries are logged with their domain and
  key path instead of the raw entry.

### Fixed

- The Postgres-backed queue lifecycle functions are compiled only with
  the `pg` feature, so the crate builds with `default-features = false`.
- `DeliveryWorker::run` stops when the shutdown sender is dropped.
  Previously a dropped sender left the loop spinning.

## [4.1.0] - 2026-08-04

### Added

- `dkim_env::extra_keys_from_env()` — builds the per-domain DKIM key map
  for `DkimSignConfig::extra_keys` from `MAILRS_DKIM_KEYS`
  (`domain:selector:path[,…]`). A malformed entry or unreadable key file
  skips that entry with a warning instead of disabling signing.
- `queue::reschedule_pending` — move a still-pending message's delivery
  time.
- Feature `spg`: the same store code on an spg-embedded pool instead of
  PostgreSQL. Implies `pg`.
- `BackendPool` type alias (`sqlx::PgPool`, or `spg_sqlx::SpgPool` with
  `spg`).

### Changed

- `PgQueueStore::new`, `PgQueueStore::pool`, the suppression-list
  functions and `DeliveryWorker::new` take or return `BackendPool`. With
  the default features this is still `sqlx::PgPool`, so existing callers
  compile unchanged.

## [4.0.0] - 2026-06-03

### Changed (BREAKING)
- `KevyNotifier::new(url: impl Into<String>)` →
  `KevyNotifier::new(store: kevy_embedded::Store)`. The notifier now drives
  the in-process kevy bus directly — no RESP wire, no network hop. `Store:
  Clone` so callers typically pass a clone of the application's shared store.
- `queue::notify` is now sync and takes `&kevy_embedded::Store` instead of
  `&mut redis::aio::ConnectionManager`.
- `DeliveryWorker::with_kevy(url: String)` →
  `DeliveryWorker::with_kevy(store: kevy_embedded::Store)`.
- Feature `pg` now pulls `kevy-embedded` instead of `redis`. No more
  `redis` transitive dependency.
- Worker's kevy `queue:notify` listener moved from an async `redis::aio`
  pubsub stream to a dedicated OS thread blocking on
  `kevy_embedded::Subscription::recv` — same wakeup semantics, no tokio
  blocking-pool slot consumed.

### Migration
```toml
# before
mailrs-outbound-queue = "3"
redis = "1"
# after
mailrs-outbound-queue = "4"
kevy-embedded = "1.1"
```
```rust
// before
let notifier = KevyNotifier::new("redis://kevy:6379");
let worker = DeliveryWorker::new(...).with_kevy("redis://kevy:6379".into());
// after
let store = kevy_embedded::Store::open(kevy_embedded::Config::default())?;
let notifier = KevyNotifier::new(store.clone());
let worker = DeliveryWorker::new(...).with_kevy(store);
```

## [3.0.0] - 2026-06-03

### Changed (BREAKING)
- Public struct `RedisNotifier` renamed to `KevyNotifier`. Update imports:
  `use mailrs_outbound_queue::KevyNotifier;` (and the path re-export at
  `pg_store::KevyNotifier`).
- Documentation refers to kevy (<https://github.com/goliajp/kevy>); the
  `redis://` URL scheme stays because it's the RESP wire identifier, not
  a backend name. `KevyNotifier::new(url: "redis://kevy:6379")` is the
  drop-in replacement for `RedisNotifier::new(...)`.
- Integration test fixtures renamed for consistency: file
  `tests/redis_notifier_integration.rs` → `tests/kevy_notifier_integration.rs`,
  helper `tests/common/redis.rs` → `tests/common/kevy.rs`, function
  `start_redis()` → `start_kevy()`. `testcontainers_modules::redis::Redis`
  remains because that's a third-party API.

## [2.0.0] - 2026-05-29

### Changed (BREAKING)

- Depends on `mailrs-smtp-client` 2. Types from it that appear in this
  crate's API (`TlsOutcome` in `TlsAttemptOutcome::HandshakeFailed`) are
  the 2.x types.
- `DkimSignConfig` has a new public field `parsed_key` (the RSA key is
  parsed once and cached) and now derives `Default`; struct literals
  need `..Default::default()`.
- DKIM signing and ARC sealing use `mailrs-dkim` 3 and `mailrs-arc` 3
  instead of `mail-auth`. Signatures are equivalent; RSA signing is
  several times faster.
- `queue::dequeue`, `queue::mark_bounced` and
  `queue::cancel_pending_by_message_id` changed signature; the
  suppression-list functions and `is_hard_bounce` moved to
  `queue::suppression` (still re-exported from `queue`).
- `worker` is a directory module. `TlsPolicy`, `try_deliver_via_mx`,
  `try_deliver_via_mx_with_tls` and `deliver_domain_static` are public;
  `try_deliver_via_mx_with_tls` takes an optional
  `rustls::ClientConfig`, so tests can inject their own verifier
  (`None` in production).

### Added

- `queue::claim_for_delivery` — atomic `FOR UPDATE SKIP LOCKED` claim, so
  several workers can share one queue.
- `queue::count_pending`, `queue::count_inflight`.
- Metrics (`metrics` crate): queue depth and delivery latency.
- Per-message ARC sealing and DKIM signing run concurrently within a
  domain batch.

### Changed

- DSNs (`dsn::format_dsn`) are built with `mailrs-mail-builder` as
  `multipart/report`; the `message/delivery-status` part is sent as 7bit
  rather than base64, and `To:` is rendered as a bare address.

## [1.2.0] - 2026-05-23

### Added

- `DeliveryEvent::TlsAttempt { domain, mx_host, outcome }`, emitted once
  per MX connection after the STARTTLS step, with a structured
  `TlsAttemptOutcome` (`Success { policy }`, `NotAdvertised`,
  `Rejected { code, message }`, `HandshakeFailed(TlsOutcome)`) suitable
  for TLS-RPT (RFC 8460) reports.

### Changed

- Requires `mailrs-smtp-client` 1.1.

## [1.1.1] - 2026-05-23

### Added

- `tracing::instrument` on `deliver_domain_static` — emits an
  `info_span!("outbound.deliver_domain", domain, n_messages,
  max_per_conn)` for each per-domain delivery batch. Combined with the
  existing log lines inside the worker, distributed-trace viewers now
  show one span per domain delivery attempt instead of correlating
  loose log lines by timestamp.

No public API change.

## [1.1.0] - 2026-05-23

### Changed

- **`retry::retry_delay_secs` now backed by `mailrs-backoff`** —
  internal switch from the hardcoded 8-slot array to
  `Backoff::smtp_outbound` (initial 60s, 2.5× growth, 8h cap).
  Schedule curve shifts slightly in attempts 1-5; cap unchanged at
  attempt 7+.

  Old: `[60, 300, 900, 1800, 3600, 7200, 14400, 28800]`
  New: `[60, 150, 375, 937, 2343, 5859, 14648, 28800]`

  Tests that asserted exact pre-1.1 values updated. If your
  deployment requires the exact old curve, pin to 1.0.2 or compute
  your own delays from a custom `mailrs_backoff::Backoff`.

### Added

- `retry::retry_delay_secs_jittered(attempt, seed)` — same curve with
  Full jitter applied. Use this in production schedulers to spread
  retry traffic and avoid synchronized retry bursts when many queue
  rows fail simultaneously (MX outage, DNS hiccup).
- `mailrs-backoff` runtime dep.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Moved `mod tests` to the end of the file so production code no longer follows the test module; removed the `#[allow(clippy::items_after_test_module)]` workaround.
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Deeper unit-test coverage and criterion benches for DKIM signing and DSN-formatting helpers.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Outbound mail queue primitives: DKIM signing, DSN generation, MTA-STS lookup, DANE TLSA verification, scheduled send, undo send, bounce processing with suppression list, and retry/backoff — with a pluggable store trait and a PostgreSQL reference implementation.
