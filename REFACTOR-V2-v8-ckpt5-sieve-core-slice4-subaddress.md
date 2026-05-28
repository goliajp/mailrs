# v8 ckpt 5 (slice 4) — sieve-core · **RFC 5233 subaddress (`:user` / `:detail`)**

Fourth slice of ckpt 5. Closes the last `:user`-aliased-to-`:localpart`
carve-out and adds full RFC 5233 subaddress support, aligned with the
swap-time oracle (sieve-rs → mail-parser).

## Motivation

Pre-slice-4 `:user` was a silent alias to `:localpart` (see
slice 5.3 doc, "capabilities" table footnote). RFC 5233 §5.1 actually
requires:

- `:user` = local-part **minus** the detail sub-part and the `+`
  joining delimiter (e.g. `"alice"` from `alice+work@example.com`)
- `:detail` = the detail sub-part (e.g. `"work"`)

Both tags are gated by the `subaddress` capability. Slice 5.3 was
already advertising `subaddress` as supported (so corpus rows like
`require ["subaddress"]; keep;` parsed cleanly) — but `:user` quietly
returned the full local-part and `:detail` was a stub returning the
empty string. Slice 4 makes the behaviour actually match the spec
**and** the oracle.

## Key design decision: align with mail-parser, not strict RFC

Differential testing surfaced two divergences between a literal
RFC 5233 §5.2 implementation and `mail-parser`'s
`parse_address_user_part` / `parse_address_detail_part` (which is
what `sieve-rs` calls). We chose to mirror mail-parser, because:

1. **Wrapper-swap safety.** Ckpt 6 will route `crates/sieve/` to
   `mailrs-sieve-core`. If sieve-core's subaddress behaviour
   silently disagrees with sieve-rs's, prod scripts that worked
   yesterday could change semantics. The point of the parity ramp
   is for ckpt 6 to be uneventful.
2. **Real-world JP mail.** `alice+work@example.com` filters are
   common; both engines agree on this single-`+` case. The diffs
   are at the multi-`+` and no-`+` corners, which are user-rare.
3. **Strict RFC interpretation is recoverable.** A caller policy
   layer above sieve-core can re-introduce the §5.2 `:is ""`
   exception if a future deployment requires it — but baking
   strict mode in would have made the wrapper swap riskier.

### `:user` algorithm (mirrors mail-parser)

Split the local-part on the **first** `+`:

```
parse_address_user_part("alice+work@example.com")        → "alice"
parse_address_user_part("alice+work+sub@example.com")    → "alice"
parse_address_user_part("alice@example.com")             → "alice"  (no +, full local-part)
```

### `:detail` algorithm (mirrors mail-parser)

Split the local-part on the **last** `+`, returning `None` when no
`+` is present:

```
parse_address_detail_part("alice+work@example.com")      → Some("work")
parse_address_detail_part("alice+work+sub@example.com")  → Some("sub")  (last segment, NOT "work+sub")
parse_address_detail_part("alice@example.com")           → None         (undefined)
```

The asymmetry (`:user` uses first `+`, `:detail` uses last `+`) comes
straight from `mail-parser-0.11.3`'s
`fields/address.rs:407..455`. We document it in
`src/address.rs::scope_to_part` so future maintainers don't "fix" it
into symmetric behaviour.

### Strict RFC 5233 §5.2 deviations (intentional)

| spec clause                                                | strict RFC                | sieve-core (slice 4)             | reason                            |
| ---------------------------------------------------------- | ------------------------- | -------------------------------- | --------------------------------- |
| `:is ""` on undefined `:detail`                            | true (§5.2 exception)     | **false** (candidate skipped)    | matches sieve-rs                  |
| `:contains "X"` / `:matches "X"` on undefined `:detail`    | false                     | false                            | spec + oracle agree                |
| Multi-`+` `:detail` split                                  | undefined                 | last `+`                         | matches sieve-rs                  |

## What changed

### `src/address.rs`

`AddressPart` enum gains `User` and `Detail` variants. The previous
`"localpart" | "user"` tag-mapping arm is split: `:localpart` →
`LocalPart` (RFC 5228 §5.1, unchanged), `:user` → `User` (new), and
`:detail` → `Detail` (new).

`scope_to_part` signature changes from `String` to **`Option<String>`**:

```rust
pub(crate) fn scope_to_part(addr: &str, part: AddressPart) -> Option<String>;
```

`None` is only returned by `Detail` when no `+` is present; every
other variant always returns `Some`. The caller-side change is
small: `eval_address` / `eval_envelope_test` now use
`let Some(scoped) = scope_to_part(&addr, part) else { continue }`
to skip undefined candidates.

### `src/capabilities.rs`

`capability_for_tag` adds `:user` and `:detail` → `"subaddress"`.
The comment on the `subaddress` capability entry now reflects full
support (was "partial").

```rust
fn capability_for_tag(name: &str) -> Option<&'static str> {
    match name {
        "flags"           => Some("imap4flags"),
        "user" | "detail" => Some("subaddress"),
        _                 => None,
    }
}
```

This means scripts using `:user` or `:detail` without
`require ["subaddress"]` are now rejected with
`MissingCapability { feature: ":user" / ":detail", capability: "subaddress" }`.
Four new validator tests pin this behaviour.

### `src/eval/test_engine.rs`

Two call-sites updated to handle the new `Option` return:

```rust
let Some(scoped) = scope_to_part(&addr, part) else { continue };
```

No other changes; the match-type / iteration loop stays as-is.

### `tests/common/corpus/slice5_b.rs` — new file, 16 rows

Drives the differential corpus through the subaddress feature
matrix:

- single `+`: `:user` and `:detail` projections against From / To
- no `+`: `:user` returns full local-part, `:detail` skips candidate
- multi-`+`: `:user` = first segment, `:detail` = last segment
- match types: `:is`, `:contains`, `:matches` × `:user` / `:detail`
- combined with `fileinto` routing

All 16 rows green vs sieve-rs on first run after the algorithm fix.
(Two rows that originally tested strict-RFC semantics — `:is ""` on
undefined detail, and `:detail = "work+sub"` on multi-`+` — were
revised to the sieve-rs-aligned values per the policy decision
above.)

### Corpus totals

| group                | rows |
| -------------------- | ---: |
| slice 1 / 2 baseline |   32 |
| slice 3              |   33 |
| slice 4 a..g         |   95 |
| slice 5_a imap4flags |   15 |
| **slice 5_b subaddress (NEW)** |  **16** |
| envelope corpus      |   12 |
| **cumulative**       |  **245** |

Differential agreement remains at 100 % (no omissions added or
removed beyond slice 4.3's two policy carve-outs).

## Test summary

```
cargo test -p mailrs-sieve-core
  → 112 unit tests pass (+1 user-distinct test, +1 detail-tag test
                          in address.rs, +4 capabilities tests for
                          :user / :detail)
  → 2 integration tests pass (engines_agree_on_corpus,
                              engines_agree_on_envelope_corpus)
  → 1 doc-test passes

cargo build --workspace        → clean
cargo clippy --workspace --all-targets -- -D warnings → clean
```

## File-size compliance

```
src/address.rs              ~180 total  /   ~80 prod-only  ✓
src/capabilities.rs          405 total  /   180 prod-only  ✓
src/eval/test_engine.rs      193 total  /  ~155 prod-only  ✓
```

All comfortably under the 500-line hard limit (prod-only basis).

## What this unlocks

- **Final ckpt 5 carve-out (functional) closes.** The only thing
  formally left in ckpt 5 is vacation 1.0 polish, which requires
  `cargo publish` (interactive `cargo login`) and is therefore the
  user's call to run.
- **Ckpt 6 wrapper swap risk drops further.** Subaddress was a
  silent semantic gap; with this slice the gap is closed and pinned
  by differential rows.
- **Capability surface stabilizes.** `SUPPORTED` set =
  `{fileinto, reject, vacation, envelope, imap4flags, subaddress}`,
  all six fully implemented to spec or to oracle (whichever
  differential testing picked).

## Next-slice candidates

- **slice 5.5 vacation 1.0 polish** — caller-side integration (dedup
  hookup + auto-reply path) + `cargo publish` prep for
  `mailrs-sieve-core@0.2` + `mailrs-mail-builder@1.0`. **Requires
  user-driven `cargo login`** — Claude cannot run.
- **ckpt 6 wrapper swap** — `crates/sieve/` internal swap from
  `sieve-rs` to `mailrs-sieve-core`; remove deny.toml AGPL exception;
  caller-side vacation Action::Vacation handling. **Big slice**,
  needs explicit user go-ahead.

## RFC 5233 conformance ledger

| §5 clause                                                             | pre-slice-4 | slice 4          |
| --------------------------------------------------------------------- | :---------: | :--------------: |
| `:user` returns local-part minus +detail                              |      ✗      |        ✓         |
| `:detail` returns the detail sub-part                                 |      ✗      |        ✓         |
| `:user` on no-`+` local-part returns full local-part                  |      —      |        ✓         |
| `:detail` on no-`+` local-part is undefined                           |      —      |   ✓ (modelled as None / candidate-skip) |
| `:detail` `:is ""` against undefined sub-part succeeds (§5.2 carve)   |      —      |    ✗ (oracle alignment, see above) |
| Multi-`+` split: `:user` first, `:detail` last                        |      —      |        ✓         |
