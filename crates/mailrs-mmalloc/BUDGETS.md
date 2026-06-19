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
| `alloc(16) + free` | 240 ns | **1.5 µs** | M6 baseline |
| `alloc(64) + free` | 240 ns | **1.5 µs** | M6 baseline |
| `alloc(256) + free` | 250 ns | **1.5 µs** | M6 baseline |
| `alloc(1024) + free` | 254 ns | **1.5 µs** | M6 baseline |
| `alloc(4096) + free` | 259 ns | **1.5 µs** | M6 baseline |

## Large path — direct mmap

Round-trip cost is dominated by mmap (~1 µs) + munmap (~1.5 µs).
Numbers scale weakly with size (kernel work is on VMA structures,
not on data).

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| `alloc(8K) + free` | 2.8 µs | **15 µs** | M6 baseline |
| `alloc(64K) + free` | TBD | **15 µs** | M7b: measure |
| `alloc(1M) + free` | TBD | **30 µs** | M7b: measure |

## Realloc growth

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| `realloc 64 → 1M doubling chain` | TBD | **100 µs** | M7b: measure |

## Steady-state churn (hold N live, churn the next)

The hold-N pattern mirrors a real server: working set lives, edge
churn alloc/frees. Numbers should be very close to the
single-thread alloc/free baseline (cache-warm).

| Op | p95 measured | Budget (5×) | Notes |
|---|---|---|---|
| Churn with `live=8` | TBD | **1.5 µs** | M7b: measure |
| Churn with `live=64` | TBD | **1.5 µs** | M7b: measure |
| Churn with `live=512` | TBD | **2 µs** | M7b: measure |

## Concurrent (8-thread)

`tests/concurrent_stress::n_workers_random_alloc_free` runs N=8 workers
× 10 K random ops each. Wall-clock for the whole test is the gate:

| Test | Wall-clock (release) | Budget | Notes |
|---|---|---|---|
| N=8 × 10K random | < 100 ms typical | **2 s** | runs as `cargo test`, debug |
| N=16 × 50K random (`#[ignore]`) | 0.82 s | **5 s** | runs `--include-ignored --release` |
| `aba_stress` N=8 × 100K | 0.43 s | **5 s** | runs `--release` |

## Comparison axes (M7b — follow-up)

The plan's M7 acceptance criterion mentioned comparing against
**mimalloc** ("mmalloc within 2× of mimalloc on at least 3 of 4
microbenchmarks"). Skipped from this M7 to avoid adding a C
build-dep (`mimalloc-sys`) to a stone that was deliberately
0-dep beyond `mailrs-syscall`. Track as **M7b**:

- Add `mimalloc` as an optional `[dev-dependencies]` behind a
  feature flag (`bench-mimalloc`); compile only when set.
- Extend `benches/alloc.rs` with parameterised allocator backend
  via a small `BackendAlloc` trait wrapping `GlobalAlloc`.
- Report `mmalloc / mimalloc` ratio per row in this file.

A first measurement against `std::alloc::System` (glibc on Linux,
already in tree, no dep) is a cheap step toward M7b — log:

| Op | mmalloc | glibc System | Ratio | Notes |
|---|---|---|---|---|
| `alloc(16) + free` | 240 ns | TBD | TBD | M7b |
| `alloc(8K) + free` | 2.8 µs | TBD | TBD | M7b |
