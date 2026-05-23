# Changelog

All notable changes to `mailrs-backoff` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-05-23

### Added

- `benches/compare_exponential_backoff.rs` — head-to-head bench vs the
  `exponential-backoff` 2.x crate.

### Performance

Measured (criterion, M-series Mac, release, `--quick`):

| Input | mailrs-backoff | exponential-backoff 2 |
|---|---:|---:|
| single attempt, no jitter | **2 ns** | 52 ns (26×) |
| single attempt, full jitter | **3 ns** | 52 ns (17×) |
| 8-attempt chain, no jitter | **10 ns** | 79 ns (8×) |

We're a pure `base_delay(attempt: u32) -> Duration` function;
`exponential-backoff` is iterator-shaped and pays per-call setup cost.

No lib code change.

## [1.0.0] - 2026-05-23

### Added

- Initial release. Generic exponential-backoff primitive with optional
  jitter, extracted from a deduplication pass across three workspace
  internal copies (outbound-queue, auth-guard, webhook outbox — each
  had its own slightly-different schedule).
- `Backoff` struct with `initial` / `multiplier` / `max` / `jitter`
  fields.
- `Jitter` enum (None / Equal / Full) — AWS Architecture Blog
  taxonomy.
- `Backoff::base_delay(attempt)` — pure exponential, useful for
  logging the "scheduled" delay alongside the actual jittered one.
- `Backoff::delay(attempt, seed)` — full delay with configured
  jitter applied. Deterministic given the same `(attempt, seed)`
  pair — tests reproduce.
- `Backoff::should_give_up(attempt, max)` — convenience predicate.
- Three presets: `smtp_outbound` (60s/2.5×/8h/Full),
  `auth_lockout` (30min/2×/24h/None), `webhook` (60s/2×/6h/Equal).
- `Default` impl: 1s/2×/1h/Full (generic HTTP retry-ish defaults).
- Zero runtime dependencies — caller supplies the jitter seed.
- 20 inline unit tests covering: default construction, exponential
  growth, max cap, jitter=None determinism, Equal-jitter bounds,
  Full-jitter bounds, deterministic-with-same-seed, all preset
  shapes, should_give_up boundary + u32::MAX, base_delay(0) ==
  initial, decay-multiplier (< 1.0), zero-base + zero-ceiling edge,
  scale_random spread, cap holds under jitter, very-high-attempt
  no overflow, Clone + Copy work.
- `tests/perf_gate.rs` with 4 regression budgets.
- `benches/backoff.rs` with 6 criterion benchmark functions.

### Out of scope (deferred)

- Async sleep helper. You sleep with your runtime's primitive.
- Retry loop runner. This crate computes durations; the loop is
  yours.
- RNG. Bring your own seed.
- Cumulative-elapsed deadline. Wrap with one Instant check.
- Decorrelated jitter (AWS "Decorrelated Jitter" variant) — adds
  state across attempts; current `Backoff` is stateless. May be
  added in 1.x.

### Future adoption (informational, no forced migration)

- `mailrs-outbound-queue::retry` — currently a hardcoded 8-slot
  array. Could switch to `Backoff::smtp_outbound` for jitter without
  changing wall-clock schedule shape.
- `mailrs-auth-guard::lockout_duration` — currently bespoke
  exponential math. Could use `Backoff::auth_lockout` to share the
  primitive.
- `mailrs-server` webhook outbox — currently a hardcoded 8-slot
  array. Could switch to `Backoff::webhook`.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-backoff-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-backoff-v1.0.0
