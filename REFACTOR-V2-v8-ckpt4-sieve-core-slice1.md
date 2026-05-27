# v8 ckpt 4 (first slice) — mailrs-sieve-core 0.1

Net-new workspace member `crates/sieve-core/`: a native RFC 5228
Sieve interpreter, Apache-2.0 OR MIT, no AGPL dependency. The
internal engine `mailrs-sieve` (the existing 1.0 wrapper around
`sieve-rs`) will route to once parity is reached (v8 ckpt 6 swap).

**This is a first slice, not the full ckpt 4.** The v8 RFC ckpt 4
deliverable calls for RFC 5228 base + a 200-script differential
corpus against `sieve-rs`. This commit delivers RFC 5228 base + a
10-script smoke differential — about 5 % of the full corpus. The
remaining 190+ scripts and the ckpt 5 extension set (vacation,
imap4flags, mime, date, notify, environment, ereject,
mailrs:ai-category) land across subsequent slices.

**Production paths still untouched.** `mailrs-sieve` (the wrapper)
keeps routing to `sieve-rs`; the AGPL exception in `deny.toml`
stays. The swap to `mailrs-sieve-core` is ckpt 6 work after the
parity ramp-up completes.

## What got built

### Crate scaffold
- `crates/sieve-core/` workspace member, `mailrs-sieve-core` 0.1.0
- `Cargo.toml` (prod dep: `mailrs-rfc5322` + `thiserror`; dev-dep:
  `sieve-rs` 0.7 as the differential oracle, scheduled for removal
  once parity is reached)
- `README.md` + `CHANGELOG.md`

### Modules (`src/`)

| Module | Purpose | LOC |
|---|---|---:|
| `lex.rs` | RFC 5228 §2 tokenizer | ~410 |
| `ast.rs` | Command / Argument / Test / MatchType / Action types | ~90 |
| `parse.rs` | RFC 5228 §3-4 recursive-descent parser | ~360 |
| `eval.rs` | RFC 5228 §4 evaluator | ~470 |
| `lib.rs` | crate façade — `eval_script(src, msg) -> Result<Vec<Action>>` | ~40 |

All files under the 500-LOC hard limit.

### RFC 5228 §2 tokenizer coverage

- identifiers (`require`, `if`, `header`, …)
- tagged args (`:is`, `:contains`, `:matches`, `:domain`, …)
- quoted strings with `\"` and `\\` escapes
- multi-line strings (`text: … .CRLF`) with dot-stuffing
- numbers with K/M/G suffix
- punctuation (`{` `}` `[` `]` `(` `)` `;` `,`)
- line comments (`# … \n`) and block comments (`/* … */`)

### RFC 5228 §3-4 parser coverage

- top-level command list
- `require [ "fileinto", "envelope" ];`-style string lists
- `if` / `elsif` / `else` chains with nested blocks
- `allof(t1, t2, …)`, `anyof(t1, t2, …)`, `not test`
- tagged + positional + nested-test arg mix

### RFC 5228 §4 evaluator coverage

| Concept | Status |
|---|---|
| Actions: `keep` / `discard` / `fileinto` / `redirect` / `reject` | ✓ |
| Implicit keep when no action fires (§2.10.6) | ✓ |
| `stop` | partial (treats as terminator marker) |
| Tests: `header` / `address` / `size` / `exists` / `true` / `false` / `not` / `allof` / `anyof` | ✓ |
| Match-types: `:is` / `:contains` / `:matches` (glob) | ✓ |
| Address-parts: `:all` / `:localpart` / `:user` / `:domain` | ✓ |
| Comparator: implicit `i;ascii-casemap` (case-insensitive) | ✓ |
| Numeric comparator `:over` / `:under` | ✓ |
| `if`/`elsif`/`else` short-circuit | ✓ |

### Tests

- 50 inline unit tests across lex/ast/parse/eval — all green
- 1 differential test (`tests/diff_sieve_rs.rs`) running 10
  scripts × representative messages through BOTH `mailrs-sieve-core`
  and `sieve-rs`, asserting same `NormalizedAction` sequence

Corpus picked for the smoke:

| script | what it exercises |
|---|---|
| `keep;` | implicit + explicit keep |
| `discard;` | explicit discard |
| `fileinto "Junk";` | string-arg action |
| `if header :is "Subject" "spam offer" {…}` | exact header match |
| `if header :contains "Subject" "spam" {…} else {…}` | substring match + else branch |
| `if size :over 1 {…}` | size :over |
| `if size :under 100K {…}` | size :under, K suffix |
| `if exists "Subject" {…}` | exists test |

Both engines produced identical action sequences on all 10
corpus rows.

## What's NOT in slice 1 (carve-outs)

- **190+ more scripts** in the differential corpus (RFC 5228
  IANA examples + Pigeonhole / FastMail public examples + mailrs
  prod scripts). Each subsequent slice adds 30-50 more.
- **`require` enforcement**: 0.1 treats `require` as advisory; a
  later slice rejects use of un-required commands.
- **`stop` proper short-circuit**: 0.1 sets the `explicit_action`
  flag but doesn't terminate the outer block. The corpus doesn't
  exercise this difference yet.
- **Comparator-spec full RFC 4790**: 0.1 hardcodes the default
  `i;ascii-casemap`. `:comparator "i;octet"` is a slice-2 item.
- **`anyof`/`allof` short-circuit ordering with side effects**:
  not an issue today since tests are pure, but worth a slice-3
  note when we add extensions that emit events during evaluation.
- **Extensions** (vacation, imap4flags, mime, date, notify,
  environment, ereject, mailrs:ai-category): ckpt 5 work.
- **Swap of `mailrs-sieve` wrapper from `sieve-rs` to
  `mailrs-sieve-core`**: ckpt 6 work, blocked until parity ≥ 99 %.

## Lines of code

`mailrs-sieve-core` 0.1 totals roughly 1,400 LOC across the four
modules. `sieve-rs` ships ~19,300 LOC across the whole crate; the
RFC 5228 base subset we cover here is the right minimum-viable
slice to prove the engine can think in the same shape as the
oracle.

## Next slice

- Expand differential corpus to 30-50 scripts (slice 2)
- Add `require` enforcement, comparator-spec, `stop` short-circuit
- Begin RFC 5230 `vacation` (the ckpt-5 extension closest to
  prod demand)

These are independent items that don't require trigger gates
between them; the v8 RFC ckpt 4 → 5 trigger is the 200-script
corpus + workspace clippy/test green, which slice 1 partially
satisfies (clippy + test green; corpus at ~5 %).
