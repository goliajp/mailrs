# mailrs-dnsbl performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-dnsbl --test perf_gate` to check.
Run `cargo bench -p mailrs-dnsbl --bench dnsbl` for the criterion
baseline.

## Path taxonomy

`reverse_ipv4` + `dnsbl_query` + `interpret_spamhaus` run **per
inbound SMTP connection** when the DNSBL stage is enabled. They are
the CPU pieces of an otherwise DNS-bound operation; the DNS lookup
itself dominates wall-clock time (milliseconds).

`DnsblCache::check` cache-hit path runs **on every inbound connect**
once the IP has been seen in the past TTL window.

## Measured (criterion, M-series Mac, release, 100-sample median)

| Operation | Median |
|---|---:|
| `reverse_ipv4` | ~14 ns |
| `dnsbl_query` (~20-char zone) | ~25 ns |
| `interpret_spamhaus` (Sbl reply) | ~700 ps |
| `interpret_spamhaus` (non-127.x → Clean) | ~700 ps |
| `DnsblCache` is_empty + len roundtrip | ~80 ns |
| `DnsblResult` eq (Sbl == Sbl) | <1 ns |

## Regression budgets

15-30× headroom over observed P95 per `rules/rust/patterns.md`.

| Path | Budget | Observed P95 (dev) | Headroom |
|---|---:|---:|---:|
| `reverse_ipv4` | 5 µs | ~100-300 ns | ~15-50× |
| `dnsbl_query` | 5 µs | ~100-300 ns | ~15-50× |
| `interpret_spamhaus` | 1 µs | ~50-200 ns | ~5-20× |

## When to re-measure

- Switching `reverse_ipv4` from `write!` to direct `push` of decimal digits
  (would be ~3× faster but more code).
- DNSBL cache changing from `Mutex<HashMap>` to `DashMap`.
- `interpret_spamhaus` adding more variants to the match.

## What is NOT in this budget

- `check_dnsbl` — wall-clock is DNS-bound (milliseconds), no useful
  bound.
- `DnsblCache::check` MISS path — also DNS-bound.
