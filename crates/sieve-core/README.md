# mailrs-sieve-core

Native RFC 5228 Sieve interpreter for `mailrs-sieve`.

## Status

**0.1 — first slice.** Tokenizer + parser + minimal evaluator for the
RFC 5228 base. Differential-tested against `sieve-rs` on a small
script set (5-10 scripts). Full parity (200-script corpus + the
extensions in v8 ckpt 5) lands across subsequent releases.

## Why

`mailrs-sieve` currently wraps Stalwart's `sieve-rs` — a 19,330-LOC
AGPL crate that handles RFC 5228 + every extension Stalwart needs.
Mailrs has a `deny.toml` AGPL exception specifically for that
dependency. `mailrs-sieve-core` is the native replacement: same
RFC 5228 + the extensions mailrs actually delivers in production,
under Apache-2.0/MIT, no exception needed.

The wrapper crate `mailrs-sieve` is untouched in this slice — it
still routes to `sieve-rs`. Once the differential parity reaches
≥ 99 % (v8 ckpt 5 → 6 trigger), the wrapper switches over and the
AGPL exception is removed (ckpt 6).

## Scope (0.1)

- RFC 5228 §2: lexical structure (tokenizer)
- RFC 5228 §3-4: AST + recursive-descent parser
- RFC 5228 §4: minimal evaluator with `keep` / `discard` /
  `fileinto` / `redirect` / `reject` actions, `header` / `address`
  / `size` / `exists` / `true` / `false` / `not` / `allof` / `anyof`
  tests, `:is` / `:contains` / `:matches` match types

## Out of scope (later slices)

- RFC 5230 `vacation`
- RFC 5232 `imap4flags`
- RFC 5703 `mime` (per-MIME-part filtering)
- RFC 5260 `date` / `index` / `spamtest` / `virustest`
- RFC 5435 `notify`
- RFC 5183 `environment`
- RFC 6131 `reject` + `ereject`
- mailrs-extension `mailrs:ai-category`

## API

```rust
use mailrs_sieve_core::{eval_script, Action};

let script = r#"
    require ["fileinto"];
    if header :is "Subject" "spam" {
        fileinto "Junk";
    } else {
        keep;
    }
"#;
let message_bytes: &[u8] = b"Subject: spam\r\n\r\nbody\r\n";
let actions = eval_script(script, message_bytes)?;
assert_eq!(actions, vec![Action::FileInto("Junk".into())]);
```

## License

Apache-2.0 OR MIT (no AGPL).
