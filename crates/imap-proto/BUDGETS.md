# mailrs-imap-proto performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-imap-proto --test perf_gate`.

| Path | Budget | Observed P95 (release, v4 ckpt 6) | Headroom |
| --- | ---: | ---: | ---: |
| `parse_command` (complex `UID SEARCH`) | 200 µs | ~164 ns | ~1200× |
| `sequence_set_to_uids` (~2000 UIDs across 3 ranges) | 1 ms | ~5.8 µs | ~170× |
| `format_fetch` (4 items) | 50 µs | ~430 ns | ~115× |

The 4000-UID expansion is realistic for a `FETCH 1:* (FLAGS)` over a
moderately-sized INBOX. Heavier expansions ought to be paginated; if
they're not, the regression gate is the wrong layer to enforce that.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
