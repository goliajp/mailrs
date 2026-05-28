# v8 ckpt 4 (third slice) + ckpt 5 起步 — sieve-core 0.1.1

Third slice of v8 ckpt 4 work. Doubles the differential corpus
(30 → 65 scripts, all green) **and** lands the RFC 5230 `vacation`
extension as ckpt 5's first step. Includes a preparatory refactor
that pulls helpers out of `eval.rs` so the file-size limit holds
after vacation dispatch is added.

## What changed

### `src/vacation.rs` (NEW, 347 lines) — RFC 5230 vacation 0.1

Parser for the `vacation` command's argument list plus 12 inline
unit tests covering RFC 5230 §3-4 (every tag, conflict detection,
implicit-keep preservation, full `eval_script` integration).

`VacationAction` carries everything the caller needs to drive the
auto-reply:

| field | type | RFC 5230 ref |
|---|---|---|
| `reason` | `String` | §3 (required positional) |
| `period` | `Option<VacationPeriod>` | §4.1 (`:days`) + RFC 6131 (`:seconds`) |
| `subject` | `Option<String>` | §4.2 |
| `from` | `Option<String>` | §4.3 |
| `addresses` | `Vec<String>` | §4.4 |
| `mime` | `bool` | §4.5 |
| `handle` | `Option<String>` | §4.6 |

The evaluator emits `Action::Vacation(va)` but **does not** set
`explicit_action`, preserving RFC 5230 §3's "vacation does not
cancel the implicit keep" rule. A script `vacation "away";` thus
emits `[Vacation, Keep]`.

### `src/address.rs` + `src/match_str.rs` (NEW, preparatory refactor)

Extracted from `eval.rs`:

- `address.rs` (122 lines): `AddressPart` enum + `address_part_from_tags` /
  `scope_to_part` / `extract_addresses` helpers (7 inline unit
  tests, including the comma-in-quoted-display-name case).
- `match_str.rs` (85 lines): `match_string` + `glob_match`
  (5 inline unit tests covering `:is` / `:contains` / `:matches`
  edge cases).

Result: `eval.rs` shrank 576 → 503 lines, leaving room for the
~10-line vacation dispatch arm without growing the file. (`eval.rs`
is still ~3 lines over the project hard limit of 500; that's slice
1/2 inherited debt, addressed in slice 4 by extracting the
`MessageContext` / `eval_test` group into its own module.)

### `tests/diff_sieve_rs.rs` — corpus 32 → 65

33 new differential rows. All 65 corpus rows green on both
engines, runtime still milliseconds.

| group | rows added |
|---|---:|
| `:matches` glob edge cases (star middle, multi-star, `?` no extra char) | 3 |
| `address :all` + multi-header list | 2 |
| size precise boundary (at-msg-size, below-msg-size) | 2 |
| nested `if` three levels deep (match + inner miss) | 2 |
| `header :is` with string-list on both sides | 2 |
| multi-action sequences (fileinto×2, fileinto+keep) | 2 |
| implicit keep when test fires but block emits no action | 2 |
| long header value (150+ chars) `:contains` | 2 |
| empty string comparisons (`:is "Subject" ""`, missing header) | 2 |
| `allof` with nested `not` | 2 |
| `anyof` three branches, only third true | 2 |
| `address :localpart` against quoted display name with comma | 2 |
| `size :over 0` (non-empty body always true) | 1 |
| folded header `\r\n ` / `\r\n\t` continuation unfolds before `:contains` | 2 |
| case-insensitive `:contains` upper, multi-missing `exists`, combined `allof(anyof, exists)`, top-level keep after non-matching if | 4 |

### `tests/common/mod.rs` (NEW) + corpus split

To keep every function ≤ 200 lines and every file ≤ 500 lines:

- Framework (`NormalizedAction`, `ours`, `sieve_rs`) → `tests/common/mod.rs`.
- Corpus split into `corpus_slice12()` (176 lines, the original 32 rows)
  + `corpus_slice3()` (196 lines, the new 33 rows) + a 5-line
  `corpus()` aggregator.

## Trigger status table — v8 ckpt 4 → 5

| gate | status | note |
|---|---|---|
| RFC 5228 base implemented | ✓ | slices 1 + 2 |
| 200 differential scripts agree | 65 / 200 (32.5%) | slice 3 = +33 rows |
| RFC 5230 vacation 0.1 | ✓ (ckpt 5 起步) | parse + emit, caller-driven runtime |
| workspace clippy + test green | ✓ | re-verified each slice |

## Why vacation is NOT in the differential corpus

`sieve-rs` internalises vacation message-building inside the
runtime — when a vacation command executes it emits a sequence of
`Event::CreatedMessage` + `Event::SendMessage` (plus optional
`Event::FileInto` for `:fcc`). The reply body, recipient
address, and Subject prefix are all derived inside `sieve-rs`
from envelope + headers + runtime config.

`sieve-core`, by design, is a **zero-I/O stone**: it surfaces a
parsed `Action::Vacation(VacationAction)` and lets the caller
(`mailrs-sieve` wrapper or server inbound pipeline) handle
dedup, recipient detection, and reply-message building.

These abstractions don't line up — there's no clean
`NormalizedAction` that captures both shapes without
string-parsing the message body `sieve-rs` builds. So vacation
RFC 5230 spec coverage lives in `vacation.rs`'s inline unit
tests (12 tests, covering parse + dual-tag conflicts + integration
via `eval_script`), not the cross-engine corpus. This is recorded
in `CHANGELOG.md` and `vacation.rs`'s module docstring so future
contributors don't try to wire vacation into the differential
runner.

## Carve-outs → slice 4

- **`require` enforcement (strict)** — still permissive. Slice 4
  flips the default and updates the ~50 inline unit tests that
  elide `require`.
- **`:comparator "i;octet"`** — case-sensitive comparator. Not
  exercised by any current corpus row.
- **`eval.rs` 503-line residual** — slice 4 extracts the
  `MessageContext` / `eval_test` group (~70 lines combined) into
  a `tests` / `tests-engine` submodule, dropping `eval.rs` to ~430.
- **`lex.rs` 523 lines** — same slice-1 debt; slice 4 splits
  string-literal scanning out into `lex/string.rs`.

All these carve-outs leave the current 65-row corpus green; they're
not blocking ship.

## File sizes after slice 3

```
src/address.rs       122   NEW
src/ast.rs           135   +47 (VacationAction + Period + Action::Vacation)
src/eval.rs          503   -73 (extracted helpers + +vacation dispatch)
src/lex.rs           523   unchanged — slice 1 debt
src/lib.rs            45   +3 (mod + re-exports)
src/match_str.rs      85   NEW
src/parse.rs         425   unchanged
src/vacation.rs      347   NEW
tests/common/mod.rs   97   NEW
tests/diff_sieve_rs.rs 459 +176 net (33 new rows after corpus split)
```

Every function ≤ 200 lines. Every src/* file ≤ 500 lines, except
the two slice-1 inherited debts (eval.rs 503, lex.rs 523) which
slice 4 closes.

## Test result

- `cargo test -p mailrs-sieve-core`: 74 unit + 1 differential
  + 1 doctest = 76 tests green, runtime ~ ms.
- `cargo test --workspace`: green (unchanged from v1.7.41 baseline).
- `cargo clippy -p mailrs-sieve-core --all-targets -- -D warnings`:
  zero warnings.
