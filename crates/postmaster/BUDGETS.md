# mailrs-postmaster performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-postmaster --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `extract_bimi_logo_url` | 20 µs | ~200 ns | ~100× |
| `parse_mta_sts_policy` | 100 µs | ~3.5 µs | ~30× |
| `validate_tlsrpt_record` (×100) | 1 ms | ~41 µs | ~25× |
| `extract_tlsrpt_rua` (multiple URIs) | 30 µs | ~1.3 µs | ~20× |

Postmaster's surface is mostly resolver-bound (DNS lookups) and isn't
amenable to wall-clock gating. These four pure parsers cover the only
in-process hot paths worth tracking — each runs per-domain-per-check
during `check_domain`, and the budgets are sized to catch
order-of-magnitude regressions (accidental allocations, switching to
heavier parsers, adding inline validation).

`validate_tlsrpt_record` is batched 100× per sample because a single
call is below the timer floor (<100 ns).

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
