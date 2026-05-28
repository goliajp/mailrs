# v8 ckpt 5 (slice 1) — sieve-core 0.2.0 · **RFC 5232 imap4flags**

First slice of v8 ckpt 5 (extension expansion). RFC 5232
`imap4flags` lands with all 4 commands + 1 test + `:flags` tag
on `keep`/`fileinto`. 15 new differential corpus rows (all
green). One pre-existing fileinto arg-shape bug surfaced and
fixed.

## What changed

### `src/ast.rs` — Action API break

`Action::Keep` and `Action::FileInto` now carry `flags: Vec<String>`:

```rust
pub enum Action {
    Keep { flags: Vec<String> },
    Discard,
    FileInto { mailbox: String, flags: Vec<String> },
    Redirect(String),
    Reject(String),
    Vacation(VacationAction),
}
```

The change matches `sieve-rs` Event::Keep / Event::FileInto shape
(both carry `flags: Vec<String>`). Callers without imap4flags-aware
delivery can ignore the field (default empty vec). Bump to 0.2.0.

### `src/eval/mod.rs` — imap4flags command dispatchers

`EvalState` gains a `flags: Vec<String>` field — the RFC 5232
implicit flags variable. The four new arms:

| command | RFC 5232 § | effect |
|---|---|---|
| `setflag` | §4.4 | replace `state.flags` with the arg list |
| `addflag` | §4.5 | union (no duplicates) of current + arg list |
| `removeflag` | §4.6 | remove arg-list flags from `state.flags` |
| `hasflag` (test) | §5.4 | true iff any arg flag matches any held flag |

Crucially: setflag/addflag/removeflag **do NOT** set
`explicit_action = true` — they are state mutations, not delivery.

`keep` and `fileinto` arms now snapshot `state.flags.clone()` into
the emitted action. If the command carries `:flags <list>`, that
list **overrides** the implicit variable just for this action
(matches sieve-rs).

### `src/eval/test_engine.rs` — `hasflag` test

`eval_test` signature gained a `flags: &[String]` parameter. The
new `hasflag` arm scans `flags` against the arg-list with
RFC 5232 §5.4 semantics (case-insensitive `i;ascii-casemap`
default, `:matches` / `:contains` supported).

### `src/eval/mod.rs` — `first_string` fix

The argument extractor `first_string` used to return the first
`Argument::String` it found. For `fileinto :flags "\\Seen" "Inbox"`
the first string is `"\\Seen"`, not the mailbox — so the wrong
folder was used. Fix: skip `:tag <value>` pairs to find the
positional string. Surfaced by corpus row
`fileinto_with_flags_tag` (slice 5.1).

### `tests/common/corpus/slice5_a.rs` (NEW) — 15 rows

All four imap4flags command paths + `:flags` tag + `hasflag` test:

| category | rows | what it exercises |
|---|---:|---|
| **setflag** | 2 | single flag, list of flags |
| **addflag** | 2 | additive on top of setflag, on empty |
| **removeflag** | 2 | flag present, flag missing (no-op) |
| **fileinto :flags** | 3 | single, list, picked-up-from-setflag |
| **keep :flags** | 2 | explicit, picked-up-from-setflag |
| **hasflag** | 3 | match, no-match, list-any-match |
| **compound** | 1 | setflag + fileinto inside if branch |

All 15 rows agreed (after fixing `first_string`) — one engine
bug surfaced and resolved within the slice.

### `tests/common/mod.rs` — NormalizedAction update + flag sorting

`NormalizedAction::Keep { flags }` and `FileInto { folder, flags }`
mirror the new Action shape. `ours` and `sieve_rs` both sort
flag lists alphabetically (RFC 5232 doesn't specify an order).

## Cumulative corpus

| slice | rows | running total |
|---|---:|---:|
| slice 1+2 | 32 | 32 |
| slice 3 | 33 | 65 |
| slice 4.1 | 35 | 100 |
| slice 4.2 | 42 | 142 |
| slice 4.3 | 60 | 202 |
| **slice 5.1** | **15** | **217** |

## Cumulative engine bug-finding rate

| slice | new rows | engine bugs |
|---|---:|---:|
| 1+2 (baseline) | 32 | 1 |
| 3 | 33 | 0 |
| 4.1 | 35 | 2 |
| 4.2 | 42 | 0 |
| 4.3 | 60 | 0 (+2 design diffs omitted) |
| **5.1** | **15** | **1** (first_string positional skip) |

Total: **4 bugs in 217 rows = 1.8%**.

## Carve-outs → slice 5.2

- **Named flags variables**: RFC 5232 §3 allows naming a flags
  variable explicitly: `setflag "X" "$keepme"; if hasflag "X"
  "\\Seen" { ... }`. Slice 5.1 supports only the implicit form
  (no name → default variable). Most filters use implicit.
- **`require ["imap4flags"]` strict enforcement** — still
  permissive across the board. Slice 5.2 can include this with
  the broader `require` strict pass (slice 2/3 carve-out).
- **Combined extensions**: vacation + imap4flags interaction (does
  vacation pick up implicit flags? RFC 5230 doesn't say — likely no).
- **`hasflag :over` / `:under` numeric match types** — RFC 5232
  §5.4 mentions but not required. Skip.

## File sizes after slice 5.1

```
src/address.rs               122   unchanged
src/ast.rs                   145   +10 (Action variants now struct-form)
src/lib.rs                    50   +5 (doctest update)
src/match_str.rs              85   unchanged
src/parse.rs                 425   unchanged
src/vacation.rs              347   unchanged (2 inline test updates)
src/lex/mod.rs               445   unchanged
src/lex/string.rs            153   unchanged
src/eval/mod.rs              407   +42 (3 commands + flags field + tag helpers)
src/eval/context.rs           35   unchanged
src/eval/test_engine.rs      147   +16 (flags param + hasflag)
tests/common/mod.rs          120   +12 (struct NormalizedAction + sort)
tests/common/corpus/slice5_a.rs  116   NEW
```

Every file ≤ 500 lines, every function ≤ 200 lines.

## Test result

- `cargo test -p mailrs-sieve-core`: 80 unit + 1 differential
  + 1 doctest = 82 tests green.
- Workspace build green.
- `cargo clippy -p mailrs-sieve-core --all-targets -- -D warnings`:
  clean (after slice 5.1).
