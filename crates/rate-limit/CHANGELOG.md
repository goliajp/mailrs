# Changelog

All notable changes to `mailrs-rate-limit` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.4] - 2026-06-03

### Changed
- Documentation-only sync: README + BUDGETS.md refer to the in-house
  RESP-compatible KV store kevy (<https://github.com/goliajp/kevy>) in the
  "Bring your own" backend examples, replacing earlier Valkey mentions.
  Redis is preserved as a generic example because RESP-protocol backends
  are still relevant ecosystem-wide.

## [1.0.3] - 2026-05-23

### Changed

- **Internal storage rewritten to GCRA-style `AtomicU64` TAT.** The
  `InMemoryRateLimitStore` now stores `DashMap<String, AtomicU64>`
  with the per-key value holding a theoretical-arrival-time in
  monotonic nanos. The hot path is lock-free (DashMap shard read
  lock + `compare_exchange_weak`). Public API and behavior unchanged.
- Hot-path clock swapped from `SystemTime::now()` (wall-clock syscall,
  ~10 ns) to `quanta::Clock` (~3-5 ns mach_absolute_time / TSC). Same
  monotonic clock `governor` uses.
- Pre-compute `nanos_per_token` + `burst_nanos` once at construction
  so the hot path is integer ops only.

### Performance

Measured (criterion, M-series Mac, release, full-sample):

| Input | Before (1.0.2) | After (1.0.3) | governor 0.10 |
|---|---:|---:|---:|
| hot key, allowed | 31 ns | **13-16 ns** | 14-18 ns |
| cold key first-touch | 306 ns | 275-372 ns | 290-420 ns |

We now **match or slightly beat** governor on the hot path. The earlier
2.2× governor lead was governor's open-source homework — storage trick
(GCRA's u64 TAT) + fast clock (quanta) — that we hadn't done. See the
docstring on `InMemoryRateLimitStore` for the storage rationale.

### Added

- `quanta = "0.12"` as a runtime dependency.

## [1.0.2] - 2026-05-23

### Changed

- `InMemoryRateLimitStore::check_at` tries `DashMap::get_mut(&str)` before
  falling back to `entry(key.to_owned()).or_insert_with(...)`. Saves one
  `String` allocation per check on the hot path (warm key, the realistic
  case in any SMTP/IMAP frontline).

### Performance

Measured (criterion, M-series Mac, release, `--quick`):

| Input | Before | After | governor 0.10 |
|---|---:|---:|---:|
| hot key, allowed | 34 ns | **31 ns** | 14 ns |
| cold key first-touch | n/a | 306 ns | 221 ns |

Honest: governor (GCRA) still wins by 2.2× on the warm path because it can
fit its state in a single u64 and use atomic CAS. We have to lock a DashMap
entry to update both `tokens` + `last_refill_unix_secs`. If you can accept
GCRA semantics, governor is the right call. If you need strict token-bucket
semantics — the only thing that works for "5 SMTP attempts per second with
burst capacity 10" — mailrs-rate-limit is the right call.

### Added

- `benches/compare_governor.rs` — head-to-head bench against `governor` 0.10.

## [1.0.1] - 2026-05-22

### Added
- New `benches/store.rs` (4 bench groups, 7 cases) covering pure math, sync/async hot keys, and stale cleanup.
- README `## Performance` section with measured criterion medians: `evaluate_bucket` ~1.7 ns, `check_sync` hot key ~33 ns, `check` async hot key ~84 ns, `cleanup_stale(10k)` ~100 µs. M-series Mac, release profile, 100-sample.

## [1.0.0] - 2026-05-22

### Added
- Initial release. Token-bucket rate limiting trait
  (`RateLimitStore`) + in-memory reference implementation
  (`InMemoryRateLimitStore`). `&str` keys, unix-seconds time, async
  trait surface — transport-agnostic and no protocol coupling.
- Pure-math entry point (`evaluate_bucket`) exposed for backend
  authors who want to plug their own storage in without going
  through the trait.
- 28 unit tests, 8 trait-contract tests, 4 perf-gate tests.
  Documented latency budgets in `BUDGETS.md`.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-rate-limit-v1.0.1...HEAD
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-rate-limit-v1.0.0...mailrs-rate-limit-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-rate-limit-v1.0.0
