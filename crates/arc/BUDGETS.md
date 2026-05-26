# mailrs-arc — Performance Budgets

`tests/perf_gate.rs` enforces these via `cargo test`. Budgets are
intentionally loose (≥ 5-10× headroom) so they catch order-of-magnitude
regressions, not micro-perf swings (criterion handles those).

| Path | Budget | Measured (M-series Mac, release, 2026-05-23) |
|---|---:|---:|
| `ArcAuthResults::parse` | 5 µs | 21 ns |
| `ArcMessageSignature::parse` (realistic) | 10 µs | 479 ns |
| `ArcSeal::parse` (realistic) | 10 µs | 295 ns |
| `ArcChain::extract` (2-hop) | 200 µs | 3.65 µs (release) / ~55 µs (dev, loaded laptop) |

## Non-budgets

| Path | Why no budget |
|---|---|
| `verify_chain` (structural) | Doesn't loop on input length — chain ≤ 50 sets total. O(n) on a constant-bounded n. |
| `verify_chain_with_crypto` | 1.0 stub returns immediately. 1.1 will add DNS + RSA verify budgets when the crypto path lands. |

## Reproducing

```bash
cargo bench -p mailrs-arc --bench arc       # criterion numbers
cargo test  -p mailrs-arc --test perf_gate  # budget enforcement
```
