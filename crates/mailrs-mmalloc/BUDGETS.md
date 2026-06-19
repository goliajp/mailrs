# mailrs-mmalloc — per-op performance budgets

Populated in M7 after the bench suite measures the finished allocator on a
representative CI machine (Linux/arm64, mailrs's CI target). Each entry sets
the budget at **3× the measured p95** so noise doesn't false-fail the gate;
`tests/perf_gate.rs` asserts `elapsed < budget` for every op listed here.

Until M7 lands, this file exists as the per-op map for the bench suite to
fill. Anyone bumping a budget must:

1. Re-measure p95 on the same machine class (Linux/arm64).
2. Document the new measurement + the 3× headroom calculation in the row.
3. Reference the commit that changed allocator semantics in a Rationale row.

## Hot path — single thread

| Operation | p95 | Budget (3×) | Rationale |
|---|---|---|---|
| `alloc(16)` + `free` round-trip | TBD | TBD | M7 baseline |
| `alloc(64)` + `free` round-trip | TBD | TBD | M7 baseline |
| `alloc(256)` + `free` round-trip | TBD | TBD | M7 baseline |
| `alloc(1024)` + `free` round-trip | TBD | TBD | M7 baseline |
| `alloc(4096)` + `free` round-trip | TBD | TBD | M7 baseline |

## Large path

| Operation | p95 | Budget (3×) | Rationale |
|---|---|---|---|
| `alloc(8K)` + `free` (mmap+munmap) | TBD | TBD | M7 baseline |
| `alloc(64K)` + `free` (mmap+munmap) | TBD | TBD | M7 baseline |
| `alloc(1M)` + `free` (mmap+munmap) | TBD | TBD | M7 baseline |

## Realloc

| Operation | p95 | Budget (3×) | Rationale |
|---|---|---|---|
| `realloc` 64 → 1M doubling chain | TBD | TBD | M7 baseline |

## Steady-state

| Operation | p95 | Budget (3×) | Rationale |
|---|---|---|---|
| Churn with `live=8` | TBD | TBD | M7 baseline |
| Churn with `live=64` | TBD | TBD | M7 baseline |
| Churn with `live=512` | TBD | TBD | M7 baseline |

## Concurrent (8-thread)

| Operation | p95 | Budget (3×) | Rationale |
|---|---|---|---|
| Random alloc/free, mixed classes | TBD | TBD | M7 baseline |
| Cross-thread free (alloc-owner≠free-owner) | TBD | TBD | M7 baseline |

## Comparison axes (M7)

For each row above, M7 records:

- `mmalloc-current/p95`
- `glibc/p95` (linux System allocator; measured against the same harness)
- `mimalloc/p95` (vendored via `mimalloc-sys`)

mmalloc is considered "mimalloc-class" on a row if its p95 is within **2×**
of mimalloc's. Any row where mmalloc is > 2× slower than mimalloc is logged
as a follow-up in the plan's issue log.
