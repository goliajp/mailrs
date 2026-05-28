# v8 ckpt 5 (slice 3) — sieve-core · **RFC 5228 §3.2 `require` strict enforcement**

Third slice of ckpt 5. Closes the four-slice-old carve-out: `require`
is no longer advisory — it now enforces RFC 5228 §3.2 to the letter.

## Motivation

RFC 5228 §3.2 wording:

> If a script does not have a "require" line for one or more
> extensions it uses, then implementations MUST treat that script as
> containing a syntax error and refuse to execute it.
>
> If the capabilities listed in a "require" action are not all
> supported by the implementation, the implementation MUST treat the
> script as containing a syntax error and refuse to execute it.
>
> If the "require" action is used, it MUST be used before any other
> actions other than other "require" actions.

Three MUST clauses. The pre-slice-3 evaluator honored none of them —
`require` arms were `Ok(())` and the rest of the engine ran without
consulting the declared capability set. Slices 2 / 3 / 4 / 5 each
flagged this as a carve-out and moved on.

Reason for prioritizing it now (over slice 5.3 subaddress / vacation
1.0 polish / ckpt 6 wrapper swap): every ckpt-5 carve-out left
unaddressed makes the eventual prod swap (ckpt 6) riskier — strict
mode is the conformance gate that lets the wrapper claim "behaves like
sieve-rs" with confidence. Pure-mechanical, no perf impact (the check
runs once at parse time, not per message).

## What changed

### New file: `src/capabilities.rs` (~330 lines incl. tests)

A standalone module exporting one entry point:

```rust
pub(crate) fn validate(commands: &[Command]) -> Result<(), EvalError>;
```

The validator runs **after** `parse_script` and **before** any
`eval_block` work. Two passes:

1. **`collect_required`** — walks the top-level command list, accepts
   `require` actions only while no other command has been seen yet,
   rejects unknown capabilities. Returns the `HashSet<String>` of
   declared capabilities.
2. **`check_command` / `check_arg` / `check_test`** — recursively
   walks every command (including nested `if`/`elsif`/`else` blocks),
   every argument (catching `:flags` tags), and every test (catching
   `envelope` / `hasflag` and their `:flags` / future `:user` tags).
   Each extension feature triggers an `ensure_declared` check against
   the set from step 1.

Supported capabilities (the only strings `require` will accept):

| capability   | RFC          | what it unlocks                              |
| ------------ | ------------ | -------------------------------------------- |
| `fileinto`   | 5228 §4.2    | `fileinto` action                            |
| `reject`     | 5429         | `reject` action                              |
| `vacation`   | 5230         | `vacation` action (partial — `Action::Vacation` emitted, caller builds reply) |
| `envelope`   | 5228 §5.4    | `envelope` test                              |
| `imap4flags` | 5232         | `setflag` / `addflag` / `removeflag` actions, `hasflag` test, `:flags` tag on `keep` / `fileinto` |
| `subaddress` | 5233         | `:user` address-part (partial — currently aliased to `:localpart`; `:detail` lands in slice 5.4) |

Anything else in a `require` list → `EvalError::UnsupportedCapability`.

### `src/eval/mod.rs` — three new error variants + wired call site

```rust
#[non_exhaustive]
pub enum EvalError {
    Parse(ParseError),
    UnknownCommand(String),
    UnknownTest(String),
    BadArg { cmd: String, detail: String },

    // New in slice 3:
    MissingCapability {
        feature: String,    // e.g. "fileinto", "envelope", ":flags"
        capability: String, // e.g. "fileinto", "envelope", "imap4flags"
    },
    UnsupportedCapability(String),
    RequireOutOfOrder,
}
```

`#[non_exhaustive]` ensures future capability-related variants
(e.g. `:detail` enforcement in slice 5.4) don't break downstream
matchers.

`eval_script_with_envelope` now reads:

```rust
let commands = parse_script(script)?;
validate_capabilities(&commands)?;   // ← slice 3 addition
let ctx = MessageContext::new(message);
...
```

The `"require" => Ok(())` arm in `eval_command` stays as a no-op,
since `validate` has already burned everything it needed from those
commands; we leave the comment updated to clarify the upstream check.

### `src/eval/mod.rs` — 3 inline tests refitted

Three pre-existing inline tests used extensions without `require` and
relied on the advisory mode. They are now spec-compliant:

```rust
// header_contains_substring — added require ["fileinto"]
// address_domain           — added require ["fileinto"]
// reject_action            — added require ["reject"]
```

Plus 4 new **negative tests** that pin the new error modes:

```
fileinto_without_require_errors    → MissingCapability { capability: "fileinto", .. }
reject_without_require_errors      → MissingCapability { .. }
unsupported_require_errors         → UnsupportedCapability("foreverbar")
require_out_of_order_errors        → RequireOutOfOrder
```

### Corpus: no change

The differential corpus (slice12 / slice3 / slice4_a…g / slice5_a /
envelope, 229 rows total) was already RFC-compliant — every row using
`fileinto` / `reject` / `imap4flags` / `envelope` / `subaddress`
already declared the matching `require`. Strict mode landed without
a single corpus edit. The differential test stayed green on the
first run (`engines_agree_on_corpus` and
`engines_agree_on_envelope_corpus` both pass).

That fact itself is a quiet vindication of slice 2-4's discipline:
even with `require` running as advisory, the corpus had been written
to spec the whole time.

## Test summary

```
cargo test -p mailrs-sieve-core
  → 101 unit tests pass (+15 new capabilities-validator tests
                          inside capabilities.rs's #[cfg(test)] mod)
  → 2 integration tests pass (engines_agree_on_corpus,
                              engines_agree_on_envelope_corpus)
  → 1 doc-test passes

cargo build --workspace        → clean
cargo clippy --workspace --all-targets -- -D warnings → clean
```

## File-size compliance

Total LOC includes the inline `#[cfg(test)] mod tests { ... }` block;
prod-only LOC is what the file-size.md §4 carve-out #4 audit consumes
(Rust convention is to inline tests in the same file):

```
src/capabilities.rs        368 total  /  175 prod-only  ✓ (limit 500)
src/eval/mod.rs            528 total  /  315 prod-only  ✓ (limit 500)
```

Both files comfortably under the hard limit on a prod-only basis.
The total-line counts are inflated by per-file `#[cfg(test)] mod
tests` blocks, which are themselves well under 500.

## What this unlocks

- **Ckpt 6 wrapper swap risk down**. After ckpt 6 the prod inbound
  pipeline runs sieve-core; "user script forgot to `require` X" now
  fails fast with a clear error rather than silently mis-evaluating.
- **Subaddress (slice 5.4) lands cleanly**. The capability registry
  is ready to add `:user` / `:detail` → `subaddress` mapping. Slice
  5.4 just edits `capability_for_tag` and switches `address_part_from_tags`
  to emit a new `Detail` variant.
- **Vacation 1.0 polish (slice 5.5) lands cleanly**. Same — vacation
  already in the supported set.

## Next-slice candidates

- **slice 5.4 subaddress** — wire `:detail` per RFC 5233, audit
  `subaddress` capability advertisement (currently partial).
- **slice 5.5 vacation 1.0 polish** — caller-side integration test
  (dedup hookup + auto-reply path), then `cargo publish` prep for
  `mailrs-sieve-core@0.2` + `mailrs-mail-builder@1.0`.
- **ckpt 6 wrapper swap** — `crates/sieve/` internal swap from
  `sieve-rs` to `mailrs-sieve-core`, removes the deny.toml AGPL
  exception, prod release.

## RFC 5228 §3.2 conformance ledger

| §3.2 MUST clause                                                | pre-slice-3 | slice 3 |
| --------------------------------------------------------------- | :---------: | :-----: |
| Script uses unrequired extension → syntax error                 |      ✗      |    ✓    |
| `require` declares unsupported capability → syntax error        |      ✗      |    ✓    |
| `require` MUST be used before any other action                  |      ✗      |    ✓    |

All three MUSTs land in this slice. No advisory mode remains.
