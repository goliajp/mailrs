# mailrs-dkim performance budgets

Regression budgets in `tests/perf_gate.rs`. Run
`cargo test -p mailrs-dkim --test perf_gate` to check.
Run `cargo bench -p mailrs-dkim --bench dkim` for the criterion
baseline.

## Path taxonomy

DKIM `verify` runs **once per inbound message** (warm path). The
parser + canonicalization are the CPU pieces; signature verification
is RSA-SHA256 (~1-2 ms for 2048-bit keys) or Ed25519 (~50-100 µs)
and dominates wall-clock time. DNS lookup of the selector dominates
real-world latency (5-50 ms).

## Measured (criterion, M-series Mac, release)

| Operation | Median |
|---|---:|
| `DkimHeader::parse` minimal | ~700 ns |
| `DkimHeader::parse` realistic 7-tag | ~1.5 µs |
| `canonicalize_body` simple | ~70 ns |
| `canonicalize_body` relaxed | ~140 ns |
| `canonicalize_header` relaxed | ~85 ns |

## Regression budgets

`tests/perf_gate.rs` asserts these are under-budget (release).
Dev mode is allowed up to 5-10× the release median; budgets are
sized for that.

| Path | Budget | Observed P95 (dev) |
|---|---:|---:|
| `parse` minimal | 10 µs | ~3-8 µs |
| `parse` realistic | 20 µs | ~5-15 µs |
| `canon_body/simple` | 5 µs | ~500ns-2µs |
| `canon_body/relaxed` | 5 µs | ~500ns-2µs |
| `canon_header/relaxed` | 5 µs | ~500ns-2µs |

## Not in budget

- `verify` end-to-end — wall-clock dominated by DNS + RSA, not by
  our parser. No useful CPU budget.
- DNS resolver — pluggable trait, perf depends on the impl.

## When to re-measure

- Switching the `rsa` crate major version.
- Adding Ed25519 / ARC / other algorithms (1.1 added Ed25519).
- Replacing `HashMap` tag list with a smarter parser.
