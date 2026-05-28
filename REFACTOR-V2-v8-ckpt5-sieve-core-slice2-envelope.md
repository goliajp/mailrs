# v8 ckpt 5 (slice 2) — sieve-core 0.2.1 · **RFC 5228 §5.4 envelope**

Second slice of ckpt 5. RFC 5228 §5.4 `envelope` test lands with
full match-type / address-part support, plus a new
`eval_script_with_envelope` entry point. 12 new envelope-aware
differential corpus rows (all green on first run).

## What changed

### `src/ast.rs` — `Envelope` struct

```rust
pub struct Envelope {
    pub from: Option<String>,    // RFC 5321 reverse-path (MAIL FROM)
    pub to: Vec<String>,         // RFC 5321 forward-path (RCPT TO)
    pub auth: Option<String>,    // RFC 5228 §5.4 :auth identity
}
```

### `src/eval/mod.rs` — new entry point + envelope threading

```rust
pub fn eval_script(script: &str, message: &[u8])
    -> Result<Vec<Action>, EvalError>;
pub fn eval_script_with_envelope(
    script: &str,
    message: &[u8],
    envelope: &Envelope,
) -> Result<Vec<Action>, EvalError>;
```

`eval_script` keeps the simple two-arg signature and delegates to
`eval_script_with_envelope` with `Envelope::default()`. The envelope
threads through `eval_block` → `eval_command` → `eval_test` so
nested `if envelope :is "from" "..."` works.

### `src/eval/test_engine.rs` — `envelope` test arm

`eval_test` signature gains a `&Envelope` parameter. The new
`envelope` arm:

```rust
"envelope" => Ok(eval_envelope_test(t, envelope)),
```

`eval_envelope_test`:
- Reads `:all` / `:localpart` / `:domain` from tags (default `:all`).
- Reads match-type tag (`:is` / `:contains` / `:matches`).
- Iterates the first arg list (envelope part names) × envelope
  values × the second arg list (match needles).
- Lookup is case-insensitive on the part name (`"from"` / `"To"` /
  `"AUTH"` all work).

### `tests/common/mod.rs` — envelope-aware diff framework

- `EnvelopeRow` type: `(label, script, msg, &[(part, value)])`.
- `ours_with_envelope` builds `Envelope` from the slice, calls
  `eval_script_with_envelope`.
- `sieve_rs_with_envelope` mirrors: calls `ctx.set_envelope(...)`
  for each entry before running, using `sieve::Envelope::From` /
  `To` (sieve-rs has no `auth` variant — we skip auth on that side).
- Existing `ours` / `sieve_rs` keep working — they're now thin
  shims over the envelope-aware versions with empty envelope.

### `tests/common/corpus/envelope.rs` (NEW) — 12 rows

| category | rows | what it exercises |
|---|---:|---|
| **envelope from match** | 4 | `:localpart`, `:domain`, `:all`, no-match |
| **envelope to** | 1 | single recipient `:localpart` match |
| **envelope to multi** | 2 | 3-recipient list with one match, all-miss |
| **empty envelope** | 1 | no envelope provided → test returns false |
| **`:matches` glob** | 1 | `"*@example.com"` against from |
| **`:contains` partial** | 1 | `"@dest"` against to |
| **string-list of part names** | 1 | `["from", "to"]` |
| **combined with body test** | 1 | `allof(envelope, header)` |

All 12 rows agree on both engines — first run.

### `tests/diff_sieve_rs.rs` — two test functions now

```rust
#[test] fn engines_agree_on_corpus()           // 217 rows (no envelope)
#[test] fn engines_agree_on_envelope_corpus()  //  12 rows (envelope state)
```

## Cumulative corpus

| slice | rows | running total | bugs |
|---|---:|---:|---:|
| slice 1+2 | 32 | 32 | 1 |
| slice 3 | 33 | 65 | 0 |
| slice 4.1 | 35 | 100 | 2 |
| slice 4.2 | 42 | 142 | 0 |
| slice 4.3 | 60 | 202 | 0 |
| slice 5.1 | 15 | 217 | 1 |
| **slice 5.2** | **12** | **229** | **0** |

Cumulative bug rate: 4 / 229 = 1.7%.

## Why envelope rows live in a separate corpus

Standard `CorpusRow = (label, script, msg)` is 3 fields. Adding
envelope state means a 4th field (or a struct), which is a typing
change that ripples through 217 existing rows. The cleanest
extension is **two parallel test functions**: one over the
existing static-msg corpus, one over the envelope-aware corpus.
The framework reuses the same `normalize()` + `sort_flags()`
helpers, only the inputs differ.

## Carve-outs → slice 5.3 / slice 6

- **`require` strict enforcement** — still permissive. Now four
  slices old (carve-out from slice 2). High time to land.
- **`subaddress` (RFC 5233)** — `:user` already mapped to LocalPart.
  Full RFC 5233 has `:detail` (the `+suffix` part of `localpart`).
- **Wrapper swap** — `crates/sieve/` still routes to `sieve-rs`.
  ckpt 6 swaps it to `sieve-core`. With 229 corpus rows + base
  RFC 5228 + RFC 5230 vacation + RFC 5232 imap4flags + RFC 5228
  §5.4 envelope, the swap is now realistic.
- **vacation 1.0 polish** — slice 3 left at 0.1. crates.io publish +
  dedup hookup integration test.

## File sizes after slice 5.2

```
src/address.rs               122   unchanged
src/ast.rs                   163   +18 (Envelope struct)
src/lib.rs                    52   +2 (Envelope re-export)
src/match_str.rs              85   unchanged
src/parse.rs                 425   unchanged
src/vacation.rs              347   unchanged
src/lex/mod.rs               445   unchanged
src/lex/string.rs            153   unchanged
src/eval/mod.rs              424   +17 (eval_script_with_envelope + threading)
src/eval/context.rs           35   unchanged
src/eval/test_engine.rs      181   +34 (envelope arg + envelope test arm)
tests/common/mod.rs          178   +58 (envelope variants + normalize/build helpers)
tests/common/corpus/envelope.rs  101   NEW
tests/diff_sieve_rs.rs        49   +19 (second test fn)
```

Every file ≤ 500 lines, every function ≤ 200 lines.

## Test result

- `cargo test -p mailrs-sieve-core`:
  - 80 unit + 2 differential (corpus + envelope) + 1 doctest
  - = 83 tests green.
- Workspace build green.
- `cargo clippy -p mailrs-sieve-core --all-targets -- -D warnings`:
  clean.
