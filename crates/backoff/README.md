# mailrs-backoff

[![Crates.io](https://img.shields.io/crates/v/mailrs-backoff?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-backoff)
[![docs.rs](https://img.shields.io/docsrs/mailrs-backoff?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-backoff)
[![License](https://img.shields.io/crates/l/mailrs-backoff?style=flat-square)](#license)

Exponential backoff with optional jitter. Pure delay math — no I/O,
no async, no RNG dependency. Useful for any retry loop: SMTP outbound
delivery, webhook re-delivery, auth-lockout penalty, HTTP client
retries, …

Follows AWS Architecture Blog's
[Exponential Backoff and Jitter](https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/)
taxonomy:

- **None** — deterministic, simple, but causes thundering-herd at scale
- **Equal** — half fixed, half random, bounded smoothing
- **Full** — uniform random `[0, base]`, AWS's recommended default

## Quickstart

```rust
use mailrs_backoff::{Backoff, Jitter};
use std::time::Duration;

// Use a preset:
let b = Backoff::smtp_outbound();
// Or roll your own:
let b = Backoff::new(
    Duration::from_secs(1),
    2.0,                            // doubling
    Duration::from_secs(300),       // cap at 5 minutes
    Jitter::Full,
);

let seed = rand_seed();
for attempt in 0..b_max_attempts() {
    let delay = b.delay(attempt, seed);
    // ... sleep then retry ...
    if Backoff::should_give_up(attempt + 1, 10) {
        break;
    }
}
# fn rand_seed() -> u64 { 0 }
# fn b_max_attempts() -> u32 { 10 }
```

## Why bring your own seed?

This crate has zero runtime dependencies and no RNG state of its own.
That means:

- **No** transitive pull-in of `rand`, `getrandom`, `wasi-libc`, etc.
- **Deterministic tests** — pass the same seed, get the same delay
- **You control** the entropy source (cryptographic if you need it,
  cheap if you don't)

Typical seed source:

```rust,no_run
// Cheap, non-crypto — fine for jitter purposes:
let seed = std::time::Instant::now().elapsed().as_nanos() as u64;
// Or, if you already have rand in your deps:
// let seed = rand::random::<u64>();
```

For `Jitter::None`, the seed is ignored entirely.

## Presets

| Preset | initial | multiplier | max | jitter |
|---|---|---|---|---|
| `smtp_outbound` | 60s | 2.5 | 8h | Full |
| `auth_lockout` | 30min | 2.0 | 24h | None |
| `webhook` | 60s | 2.0 | 6h | Equal |

These match (or extend with jitter) the policies used inside `mailrs-*`
crates — see workspace comments. Pick a preset or tune your own; the
struct fields are all `pub`.

## What this crate does

- **`Backoff::base_delay(attempt)`** — `min(initial × multiplier^attempt, max)`,
  pure exponential without jitter. Useful for logging the "scheduled"
  delay alongside the actual jittered one.
- **`Backoff::delay(attempt, seed)`** — base delay with jitter applied
  per the configured `Jitter` policy.
- **`Backoff::should_give_up(attempt, max)`** — convenience predicate
  for retry loops.
- Three presets: `smtp_outbound`, `auth_lockout`, `webhook`.

## What this crate does not

- **No async sleep helper.** You sleep with `tokio::time::sleep` /
  `std::thread::sleep` / whatever your runtime gives you.
- **No retry loop runner.** This crate computes durations; the loop
  is yours.
- **No RNG.** Bring your own seed.
- **No "cap on total elapsed".** If you want "give up after 30
  minutes of cumulative retries," wrap with your own deadline check —
  one extra `if Instant::now() > deadline { break }` line.

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `base_delay(attempt=3)` | **~8 ns** |
| `delay(attempt=3, Jitter::None)` | **~23 ns** |
| `delay(attempt=3, Jitter::Equal)` | **~31 ns** |
| `delay(attempt=3, Jitter::Full)` | **~11 ns** |
| `delay(attempt=100, capped)` | **~10 ns** |
| `should_give_up` | **<1 ns** |

Reproduce: `cargo bench -p mailrs-backoff --bench backoff`. Workspace
[PERFORMANCE.md](../../PERFORMANCE.md) carries the same table.

## License

Apache-2.0 OR MIT.
