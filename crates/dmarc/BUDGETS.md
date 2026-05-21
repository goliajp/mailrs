# mailrs-dmarc performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-dmarc --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `generate_dmarc_report_xml` (500 records) | 30 ms | ~1.5 ms | ~20× |
| `extract_rua_from_dmarc_record` | 50 µs | ~1 µs | ~50× |

500-record report represents the upper end of typical daily DMARC volume
for a small-to-mid domain — anything more would be unusual.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
