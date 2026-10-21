# mailrs-mmalloc — per-op performance budgets

`tests/perf_gate.rs` asserts each row below: `elapsed < budget`. CI's
`cargo test --workspace` fails on regression. Budgets are set at **5×**
the measured p95 on a representative dev machine to absorb noise on
slower CI runners (criterion warns on regression but doesn't fail CI,
which is why we maintain integration perf_gate tests alongside the
bench suite).

**Reference machine**: linux/amd64 via docker on macOS arm host
(rosetta-translated; numbers conservative — native linux/amd64 is
typically ~30% faster).

Anyone bumping a budget must:

1. Re-measure p95 on the same machine class.
2. Document the new measurement + the 5× headroom calculation in the row.
3. Reference the commit that changed allocator semantics in the
   `Notes` column.

## Hot path — single-thread alloc + free round-trip

Steady-state per-iter cost of `MailrsAllocator.alloc(layout); *p =
…; MailrsAllocator.dealloc(p, layout)`. Loops past the TLAB
overflow boundary so it includes refill cost; this is the
**conservative** number, not the warm-cache best case.

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| `alloc(16) + free` | 240 ns | **1.5 µs** | measured |
| `alloc(64) + free` | 240 ns | **1.5 µs** | measured |
| `alloc(256) + free` | 250 ns | **1.5 µs** | measured |
| `alloc(1024) + free` | 254 ns | **1.5 µs** | measured |
| `alloc(4096) + free` | 259 ns | **1.5 µs** | measured |

## Large path — direct mmap

Round-trip cost is dominated by mmap (~1 µs) + munmap (~1.5 µs).
Numbers scale weakly with size (kernel work is on VMA structures,
not on data).

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| `alloc(8K) + free` | 2.8 µs | **15 µs** | measured |
| `alloc(64K) + free` | TBD | **15 µs** | not yet measured |
| `alloc(1M) + free` | TBD | **30 µs** | not yet measured |

## Realloc growth

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| `realloc 64 → 1M doubling chain` | TBD | **100 µs** | not yet measured |

## Steady-state churn (hold N live, churn the next)

The hold-N pattern mirrors a real server: working set lives, edge
churn alloc/frees. Numbers should be very close to the
single-thread alloc/free baseline (cache-warm).

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| Churn with `live=8` | TBD | **1.5 µs** | not yet measured |
| Churn with `live=64` | TBD | **1.5 µs** | not yet measured |
| Churn with `live=512` | TBD | **2 µs** | not yet measured |

## Concurrent (8-thread)

`tests/concurrent_stress::n_workers_random_alloc_free` runs N=8 workers
× 10 K random ops each. Wall-clock for the whole test is the gate:

| Test | Wall-clock (release) | Budget | Notes |
|---|---|---|---|
| N=8 × 10K random | < 100 ms typical | **2 s** | runs as `cargo test`, debug |
| N=16 × 50K random (`#[ignore]`) | 0.82 s | **5 s** | runs `--include-ignored --release` |
| `aba_stress` N=8 × 100K | 0.43 s | **5 s** | runs `--release` |

## Not yet measured

There is no comparison against **mimalloc** yet. Adding it would mean a
C build dependency (`mimalloc-sys`), and this crate has no dependencies
beyond `mailrs-syscall`. If added, it would be:

- `mimalloc` as an optional dev-dependency behind a `bench-mimalloc`
  feature;
- `benches/alloc.rs` parameterised over the allocator through a small
  wrapper around `GlobalAlloc`;
- an `mmalloc / mimalloc` ratio per row in this file.

A comparison against `std::alloc::System` (glibc on Linux) needs no new
dependency and is also still open:

| Op | mmalloc | glibc System | Ratio |
|---|---|---|---|
| `alloc(16) + free` | 240 ns | TBD | TBD |
| `alloc(8K) + free` | 2.8 µs | TBD | TBD |
