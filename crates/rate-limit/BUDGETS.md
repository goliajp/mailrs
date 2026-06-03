# mailrs-rate-limit performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set
with 15-30× headroom over the observed P95 on a dev machine so CI fails
on order-of-magnitude regressions, not micro-noise.

Run `cargo test -p mailrs-rate-limit --test perf_gate` to check.

## Path taxonomy

Rate-limit checks are **warm** path per `rules/rust/patterns.md` — one
call per inbound TCP accept (SMTP) or per HTTP request (web API).
Budgets sit comfortably below the ≤ 10 ms warm-path ceiling because a
rate-limit check that takes meaningful time defeats its own purpose:
the check exists to short-circuit work before the connection is given
real resources.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `evaluate_bucket` (pure math) | 5 µs | ~10 ns | ~500× | Three floating-point ops + a branch. Inlinable; budget mostly catches accidental allocations. |
| `InMemoryRateLimitStore::check_sync` (hot key) | 30 µs | ~1 µs | ~30× | DashMap entry-lock + `SystemTime::now()` syscall + the pure math. |
| `InMemoryRateLimitStore::check` (async, hot key) | 50 µs | ~1.5 µs | ~30× | Adds `async_trait` boxed-future overhead on top of the sync path. |
| `InMemoryRateLimitStore::check_sync` (cold key) | 50 µs | ~3 µs | ~15× | First check per key allocates a `String` and inserts into DashMap (may resize). |

The cold-key budget is the only one that costs real allocation; in
production, traffic is dominated by hot-key checks (the same client
reconnecting many times). The cold-key gate exists to catch
regressions in the construction path (e.g. switching from `to_owned()`
to a slower allocator, adding a SHA hash to the key path).

## Methodology

- Each test runs the path 100 times.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time. Tests run on any
  reasonable CI executor.
- `evaluate_bucket` is gated **in both debug and release** profiles;
  budgets allow for the ~5-10× debug-build slowdown.

## When to re-measure

Update the table (and the asserts in `perf_gate.rs`) when any of
these fire:

- Hot-path code changes — `evaluate_bucket`, `InMemoryRateLimitStore`
  internals (DashMap layout, key allocation strategy).
- A new `RateLimitStore` impl is added in this crate (e.g. a Kevy
  / Redis backend in 1.1.0).
- The CI runner changes class (we move from x86 to arm, switch to a
  GitHub-hosted runner, etc).

Never weaken a budget without recording the new observed P95 and the
reason for the slowdown in the commit message.
