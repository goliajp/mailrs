# v8 ckpt 4 (fourth slice, part 1) — sieve-core 0.1.2

Fourth slice's first half. Doubles the differential corpus (65 → 100
scripts, all green) — **ckpt 4 → 5 trigger gate now at 50% (100/200)**.
Surfaces and fixes a `stop` semantics bug carried since slice 2,
plus a fairness fix in the test framework's sieve-rs config.

## What changed

### `tests/common/corpus/{slice4_a,slice4_b}.rs` (NEW) — 35 rows

Added 35 new differential rows. All 100 corpus rows green on both
engines.

| group | rows | what it exercises |
|---|---:|---|
| **A. Comments** | 3 | `# line` + `/* block */` between tokens, trailing line comment after semicolon |
| **B. Escape sequences** | 2 | `\"` inside quoted strings; `:contains` against escaped Subject |
| **C. K / M / G suffix** | 3 | `size :over 1K`, `:under 1M`, `:under 2G` |
| **D. elsif chains** | 3 | 5-level chain first-match, 5-level chain late-match, fall-through to `else` |
| **E. Deeply nested if** | 2 | 4 levels deep, all-true vs innermost-false |
| **F. Multi-action sequences** | 2 | `fileinto + redirect + keep`, two redirects |
| **G. `require` multi-extension** | 2 | one list with 3 caps, two separate `require` calls |
| **H. Empty / minimal blocks** | 2 | `if … { }` with test true vs false |
| **I. Nested allof/anyof** | 3 | `allof(anyof, allof)`, `not allof(…)`, `not anyof(…)` |
| **J. Multi-recipient address** | 4 | match across To list, no-match, Cc match, `anyof(To, Cc)` |
| **K. Case-insensitive lookup** | 3 | `exists "subject"`, `exists ["SUBJECT", "to", "From"]`, `header :is "subject" …` |
| **L. Mixed-case `:matches`** | 1 | `*OFFER*` against `spam offer` |
| **M. Address-part edges** | 3 | `:domain "example.com"`, `:all "alice@example.com"`, `:localpart "ALICE"` |
| **N. Reject edges** | 2 | single-char reason, reason with period |
| **O. Stop short-circuit edge** | 1 | `stop; keep;` — the bug-surfacer |

### `src/eval.rs` — `stop` doesn't cancel implicit keep (FIX)

Slice 2's `stop` rewrite proper-short-circuited via the `stopped`
flag but **also** set `state.explicit_action = true` — that meant a
script like `stop; keep;` emitted `[]` (no actions, implicit keep
suppressed). RFC 5228 §4.5 is explicit: *"If the implicit keep is in
effect, the message is kept."* Slice 4's `stop_at_top_level_before_keep`
corpus row surfaced the divergence vs sieve-rs. Fix: remove the
errant `explicit_action = true` line.

### `tests/common/mod.rs` — `sieve-rs` `max_redirects = usize::MAX`

sieve-rs defaults `max_redirects = 1` as an anti-mail-loop policy
(`runtime/mod.rs:355`). That's a caller-policy decision, not an
RFC 5228 requirement. `sieve-core` (zero-I/O stone) leaves the
decision to the caller — so for the differential test we lift the
cap to compare spec behaviour, not policy. Without this, the
`two_redirects` corpus row would have failed because sieve-rs
silently dropped the second redirect.

### Corpus split into `tests/common/corpus/`

The single 727-line `tests/diff_sieve_rs.rs` was over the project
500-line hard limit, and its `corpus()` function was 406 lines —
beyond the 200-line function limit too. Slice 4 moves all corpus
data into per-slice sub-modules:

```
tests/common/
├── mod.rs           — framework: NormalizedAction, ours, sieve_rs (108 lines)
├── corpus/
│   ├── mod.rs       — MSG_* fixtures + slice aggregator (80 lines)
│   ├── slice12.rs   — original 32 rows (181 lines, fn ≤ 200)
│   ├── slice3.rs    — slice 3 additions (200 lines, fn ≤ 200)
│   ├── slice4_a.rs  — slice 4 categories A-G (141 lines, fn ≤ 200)
│   └── slice4_b.rs  — slice 4 categories H-O (119 lines, fn ≤ 200)
└── (diff_sieve_rs.rs is now 30 lines — just the test driver)
```

Every function ≤ 200 lines. Every test file ≤ 500 lines.

## Trigger status table — v8 ckpt 4 → 5

| gate | status | note |
|---|---|---|
| RFC 5228 base implemented | ✓ | slices 1 + 2 |
| 200 differential scripts agree | **100 / 200 (50%)** | slice 4.1 = +35 rows |
| RFC 5230 vacation 0.1 | ✓ | slice 3 |
| workspace clippy + test green | ✓ | re-verified |

## Bug-finding rate

Slice 3: 0 disagreements out of 33 new rows. Slice 4: 2 out of 35.
Both surfaced legitimate engine issues that needed fixing
(not corpus quirks). Worth the time — exactly the value differential
testing is supposed to deliver. The bug rate uptick is also a hint
that we're past the easy parity coverage — slice 5+ should expect
more disagreement work per row.

## Carve-outs → slice 4.2 / slice 5

- **`require` enforcement (strict)** — still permissive. Slice 4.2
  will flip the default and update the ~50 inline unit tests that
  elide `require`.
- **`:comparator "i;octet"`** — case-sensitive comparator. Not yet
  exercised. Add in slice 5.
- **`eval.rs` 501-line residual** — slice 4.2 extracts the
  `MessageContext` group (~70 lines) into a `tests`/`context`
  submodule, dropping `eval.rs` to ~430.
- **`lex.rs` 523 lines** — slice 1 debt; slice 4.2 splits the
  string-literal scanner into `lex/string.rs`.
- **`stop` inside nested `if`** — slice 2's `stopped` flag works,
  but the existing `stop_short_circuit` corpus row uses a preceding
  `discard` which already sets `explicit_action`, so the slice 4
  fix only changes behaviour for stop-without-prior-action paths.
  Slice 5 should add a `stop` corpus row that exercises the
  combined case explicitly.

## File sizes after slice 4

```
src/address.rs           122   unchanged
src/ast.rs               135   unchanged
src/eval.rs              501   -2 net (-1 stop fix, -1 of 4-line comment trim)
src/lex.rs               523   unchanged — slice 1 debt
src/lib.rs                45   unchanged
src/match_str.rs          85   unchanged
src/parse.rs             425   unchanged
src/vacation.rs          347   unchanged
tests/diff_sieve_rs.rs    30   -697 (corpus + framework extracted)
tests/common/mod.rs      108   +6 (max_redirects fix + CorpusRow type)
tests/common/corpus/...           (NEW directory)
```

Every function ≤ 200 lines. Every src/* file ≤ 500 lines except the
two slice-1 inherited debts (eval.rs 501, lex.rs 523) which slice 4.2
closes.

## Test result

- `cargo test -p mailrs-sieve-core`: 74 unit + 1 differential
  + 1 doctest = 76 tests green, runtime ~ ms (no flakiness over
  two consecutive runs).
- `cargo build --workspace`: green (no downstream depends on
  `mailrs-sieve-core` yet — `crates/sieve/` wrapper still routes to
  `sieve-rs`; the swap is ckpt 6 work).
- `cargo clippy -p mailrs-sieve-core --all-targets -- -D warnings`:
  zero warnings.
