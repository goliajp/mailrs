# v8 ckpt 4 (slice 4.2) — sieve-core 0.1.3

Slice 4.2: corpus 100 → 142 scripts (**71% trigger parity**), full
closure of slice 1/2's inherited file-size debt (`lex.rs` 523 +
`eval.rs` 501), all 0/42 disagreements on new rows.

## What changed

### `src/lex/` (directory restructure) — 523-line lex.rs split

- `src/lex.rs` (523 lines, `tokenize` 235 lines) → `src/lex/mod.rs`
  (445 lines, `tokenize` 164 lines) + `src/lex/string.rs` (153 lines).
- Extracted `scan_quoted` + `scan_multiline` + `is_multiline_start`
  + their UTF-8 helper into `lex/string.rs` with 6 dedicated unit
  tests (single-quote, escaped-quote, unterminated, simple
  multi-line, dot-stuffing, unterminated multi-line).
- `tokenize` loop in `lex/mod.rs` now delegates the two string
  shapes to those helpers — much smaller dispatch loop, easier to
  reason about, and under the function-size limit.

### `src/eval/` (directory restructure) — 501-line eval.rs split

- `src/eval.rs` (501 lines) → `src/eval/mod.rs` (365 lines) +
  `src/eval/context.rs` (35 lines) + `src/eval/test_engine.rs`
  (131 lines).
- `context.rs`: `MessageContext` (raw bytes wrapper) with
  `new` / `header_values` (RFC 5322 §2.2.3 unfolding) / `body_size`.
- `test_engine.rs`: `eval_test` dispatch + `eval_size` / `eval_header`
  / `eval_address` plus the `pair_lists` / `arg_strings_or_list`
  arg-shaping helpers that only test eval uses.
- `mod.rs` now contains only the command-dispatch layer
  (`eval_script`, `eval_block`, `eval_command`) plus inline tests
  covering the full command surface (21 tests).

### `tests/common/corpus/slice4_c.rs` + `slice4_d.rs` (NEW) — 42 rows

| group | rows | what it exercises |
|---|---:|---|
| **P. Multi-line `text:` strings** | 3 | reject reason / fileinto arg / dot-stuffing in `text:` block |
| **Q. Number edges** | 3 | `size :under 1`, very large number, exact 1024 |
| **R. Whitespace tolerance** | 3 | many blank lines, mixed tabs/spaces, multi-action one-liner |
| **S. Header-value edges** | 4 | empty Subject `:is ""`, empty `:contains`, `:matches "*"`, `:matches "?"` |
| **T. Address shape edges** | 3 | localpart with dots, subdomain match, subdomain ≠ parent |
| **U. Message shape edges** | 2 | no-body size, no-body exists `Subject` |
| **V. Comments in unusual positions** | 2 | `#` between if-test and action, `/* */` inside `allof(…)` |
| **W. Deep nesting variants** | 4 | 3-branch allof short-circuit, anyof+not, not+size, nested allof in anyof |
| **X. Action sequence semantics** | 4 | fileinto+keep, discard alone, stop in else, stop in then blocks outer |
| **Y. require edges** | 3 | require-only (no action), require+keep, two separate require calls combined |
| **Z. RFC compliance specifics** | 4 | leading space in value, case-insensitive localpart, exists+not, elsif-after-anyof |
| **AA. Real-world filter shapes** | 3 | newsletter pattern, VIP priority pattern, auto-archive |
| **BB. Misc** | 4 | exists with middle missing, empty script, comment-only script |

All 42 new rows agreed on **first run** — no engine disagreements
this slice. Cumulative bug-finding rate now 2/142 (1.4%).

## Trigger status table — v8 ckpt 4 → 5

| gate | status | note |
|---|---|---|
| RFC 5228 base implemented | ✓ | slices 1 + 2 |
| 200 differential scripts agree | **142 / 200 (71%)** | slice 4.2 = +42 rows |
| RFC 5230 vacation 0.1 | ✓ | slice 3 |
| workspace clippy + test green | ✓ | re-verified |
| file-size hard limit (500 / 200) | ✓ closed | slice 1/2 debt cleared |

## File sizes after slice 4.2

```
src/address.rs               122   unchanged
src/ast.rs                   135   unchanged
src/lib.rs                    45   unchanged
src/match_str.rs              85   unchanged
src/parse.rs                 425   unchanged
src/vacation.rs              347   unchanged
src/lex/mod.rs               445   -78 (was 523, single file)
src/lex/string.rs            153   NEW
src/eval/mod.rs              365   -136 (was 501)
src/eval/context.rs           35   NEW
src/eval/test_engine.rs      131   NEW
tests/common/corpus/*         all ≤ 200 lines per slice file
```

**Every file ≤ 500 lines. Every function ≤ 200 lines.** First time
since slice 1.

## Carve-outs → slice 5

- **`require` enforcement (strict)** — still permissive. Continue
  to push to slice 5 to keep the corpus expansion linear without
  forcing the ~50 inline-test rewrite mid-slice.
- **`:comparator` tags** — `:comparator "i;ascii-casemap"` is the
  default and shouldn't need explicit handling; `:comparator "i;octet"`
  (case-sensitive) is a real gap, but no current corpus row
  exercises it.
- **`envelope` test** — `if envelope :is "to" "alice@x"` needs
  envelope context. sieve-rs supports it; sieve-core would need a
  context-passing mechanism (caller supplies `MAIL FROM` / `RCPT TO`).
- **`imap4flags`** — `fileinto :flags ["\\Seen"]` etc. UI value high
  (mailbox flags surface in mailrs's mailbox view), but a bigger
  architectural change because flags must travel with the action.

## Test result

- `cargo test -p mailrs-sieve-core`: 80 unit + 1 differential
  + 1 doctest = 82 tests green (6 new from `lex/string.rs`).
- `cargo build --workspace`: green.
- `cargo clippy -p mailrs-sieve-core --all-targets -- -D warnings`:
  zero warnings.
