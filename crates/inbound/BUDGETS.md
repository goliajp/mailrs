# mailrs-inbound performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set with
15-30× headroom over the observed P95 on a dev machine so CI fails on
order-of-magnitude regressions, not micro-noise.

Run `cargo test -p mailrs-inbound --test perf_gate` to check.

## Path taxonomy

The inbound pipeline is a **warm** path — per-message latency, not
per-frame. Per `rules/rust/patterns.md`: warm budgets sit ≤ 10 ms total.
Every path below runs once per inbound SMTP DATA transaction, on the
critical line between the client's `.\r\n` and the 250/451/550 response.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `make_delivery_decision` (Accept) | 30 µs | ~1.1 µs | ~30× | Pure policy combiner + auth_header build |
| `make_delivery_decision` (Junk) | 50 µs | ~1.8 µs | ~30× | Extra `format!` for score breakdown + matched rules |
| `make_delivery_decision` (DMARC Reject) | 30 µs | ~1.3 µs | ~25× | Builds auth_header even on reject path |
| `build_auth_header` | 20 µs | ~1.1 µs | ~20× | 4-result vec + format |
| `build_auth_header` (with reason) | 20 µs | ~1.3 µs | ~15× | Extra `reason="..."` write |
| `format_auth_results_header` | 20 µs | ~0.7 µs | ~30× | Bare value + `Authentication-Results: ` prefix |
| `ReceiveContext::to_pipeline_input` | 5 µs | ~125 ns | ~30× | Clones AuthResults + rules + hostname per message |

`Pipeline::run` itself is not gated — it dispatches dynamic `Stage` impls
whose cost is owned by the downstream stage (DNS resolver, ClamAV TCP,
LLM API), not by this crate. The stages' costs are out of process /
network-bound and live outside this budget table.

## Methodology

- Each test runs the path 100 times.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time. Tests must be runnable on
  any reasonable CI executor; we don't pin to high-resolution clocks
  or special hardware.

## When to re-measure

Update the table (and the asserts in `perf_gate.rs`) when any of these
fire:

- Hot-path code changes — `make_delivery_decision`, `build_auth_header`,
  `format_auth_results`, or `ReceiveContext::to_pipeline_input`.
- A new `Stage` impl in mailrs-inbound itself (the framework crate; not
  downstream consumer stages).
- The CI runner changes class (we move from x86 to arm, etc).

Never weaken a budget without recording the new observed P95 and the
reason for the slowdown in the commit message.
