# mailrs-clean performance budgets

Enforced by `tests/perf_gate.rs`. Catches order-of-magnitude regressions
on the html-clean + quoted-reply hot paths. Run
`cargo test -p mailrs-clean --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `clean_email_html` (marketing) | 5 ms | ~250 µs | ~20× |
| `detect_bulk_sender` | 50 µs | ~2 µs | ~25× |
| `split_quoted_content` | 500 µs | ~20 µs | ~25× |

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
