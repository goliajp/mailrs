# Changelog

All notable changes to `mailrs-shield` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Features split: `store` (`GreylistDb` over any `GreylistBackend`),
  `kevy-store` (adds `EmbeddedGreylistBackend` and `GreylistDb::new`),
  `pg` (the PostgreSQL cold backup, `GreylistDb::with_pg`), `spg`
  (implies `pg`). The default is `["kevy-store", "pg"]`, which matches
  what the old default enabled.
- **Breaking for non-default feature sets:** `kevy-store` no longer pulls
  in `sqlx`; a build with `default-features = false,
  features = ["kevy-store"]` that calls `with_pg` must add `pg`.
  `GreylistDb::with_backend` is available without any kevy dependency.

## [3.1.0] - 2026-08-04

### Added

- `GreylistBackend` trait for the first-seen store, with
  `EmbeddedGreylistBackend` over `kevy_embedded::Store`, and
  `GreylistDb::with_backend(Arc<dyn GreylistBackend>)` to plug in another
  backend such as a shared network kevy-server. Backend errors fail open:
  a failed read counts as "not seen", a failed write is dropped.
- Feature `spg`: the cold-backup pool is spg-embedded instead of
  PostgreSQL. `BackendPool` names the active pool type.

### Changed

- `GreylistDb::with_pg` takes `BackendPool` (`sqlx::PgPool` with the
  default features).
- On a kevy miss with a PostgreSQL pool attached, the first-seen value is
  loaded from PostgreSQL and written back to kevy, so senders already
  recorded are not greylisted again after the kevy data is lost.

## [3.0.0] - 2026-06-03

### Changed (BREAKING)
- `GreylistDb::new(conn: redis::aio::ConnectionManager)` →
  `GreylistDb::new(kevy: kevy_embedded::Store)`. The store is now driven
  directly through the in-process kevy [`Store`] handle, dropping the
  RESP wire entirely. `Store: Clone` so callers typically pass a clone
  of the application's shared store.
- Feature `kevy-store` now pulls `kevy-embedded` instead of `redis`. No
  more `redis` transitive dependency.
- All store ops (`get` / `set_with_ttl` / `expire`) switched to the
  sync `kevy_embedded::Store` surface; the public `check(..) -> async`
  signature is unchanged so the only break is the constructor.

### Migration
```toml
# before
mailrs-shield = "2"
redis = "1"
# after
mailrs-shield = "3"
kevy-embedded = "1.1"
```
```rust
// before
let cm = redis::aio::ConnectionManager::new(redis::Client::open("redis://…")?).await?;
let db = GreylistDb::new(cm);
// after
let store = kevy_embedded::Store::open(kevy_embedded::Config::default())?;
let db = GreylistDb::new(store);
```

## [2.0.0] - 2026-06-03

### Changed (BREAKING)
- Feature flag `redis-store` renamed to `kevy-store`. Update your `Cargo.toml`:
  `mailrs-shield = { version = "2", features = ["kevy-store"] }`.
- Internal `GreylistDb` field/parameter rename `valkey` → `kevy`. The public
  `GreylistDb::new(conn: redis::aio::ConnectionManager, ...)` signature still
  takes a `redis::aio::ConnectionManager` (RESP wire protocol unchanged);
  only the parameter binding name changed, so callers using positional
  arguments are unaffected.
- Documentation refers to the RESP-compatible KV store kevy
  (<https://github.com/goliajp/kevy>) instead of Valkey/Redis. The
  `redis://` URL scheme stays because it identifies the wire protocol,
  not the backend product.

## [1.0.4] - 2026-05-22

### Changed

- The `dnsbl` module re-exports `mailrs-dnsbl` (new dependency) instead of
  carrying its own copy. The public items under `mailrs_shield::dnsbl` are
  unchanged.

## [1.0.3] - 2026-05-22

### Changed

- `triplet_key` builds the key in a pre-sized buffer instead of with
  `format!`. Output is unchanged.

## [1.0.2] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `interpret_spamhaus` ~700 ps, `ptr_score_from_names` ~85 ns, `triplet_key` ~120 ns. M-series Mac, release profile, 100-sample.

## [1.0.1] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.0] - 2026-05-20

### Added
- Initial release. SMTP server anti-spam primitives in three modules: DNS blocklist (DNSBL) queries, greylisting policy with an optional Redis store, and PTR / forward-confirmed reverse DNS (FCrDNS) checks.
