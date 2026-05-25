# mailrs-sieve

[![Crates.io](https://img.shields.io/crates/v/mailrs-sieve.svg)](https://crates.io/crates/mailrs-sieve)
[![Docs.rs](https://docs.rs/mailrs-sieve/badge.svg)](https://docs.rs/mailrs-sieve)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

**Clean delivery-action wrapper over Stalwart's `sieve-rs`** (RFC 5228
Sieve email filtering + the common extensions: `fileinto`,
`vacation`, `reject`, `redirect`, …).

`sieve-rs` is a complete Sieve implementation but exposes a
low-level event-loop API — every MTA has to write the same
translation layer to turn `sieve::Event` variants into actual
delivery decisions. `mailrs-sieve` IS that layer:

- Flat `SieveAction { Keep, FileInto, Discard, Redirect, Reject,
  Vacation }` enum the caller pattern-matches in its delivery loop
- `created_messages` tracking so `vacation` / `notify` reply bodies
  are correctly paired with their `SendMessage` events
- Envelope `From`/`To` injection for `:envelope` tests + vacation
  auto-reply addressing

Pure compile + evaluate. No I/O, no async. Plug into any
script-storage layer (PG, file, in-memory) and any delivery
backend (Maildir, IMAP, JMAP).

## Quick start

```rust
use mailrs_sieve::{compile_sieve, evaluate_sieve_with_envelope, SieveAction};

let script = "fileinto \"INBOX/spam\";";
let compiled = compile_sieve(script).unwrap();

let message = b"From: a@b.c\r\nSubject: t\r\n\r\nbody";
let actions = evaluate_sieve_with_envelope(
    &compiled,
    message,
    Some("a@b.c"),
    Some("me@d.e"),
);

match &actions[0] {
    SieveAction::FileInto(folder) => assert_eq!(folder, "INBOX/spam"),
    _ => unreachable!(),
}
```

## API

| Function | Purpose |
|---|---|
| `compile_sieve(script)` | Parse + compile a Sieve script to an `Arc<CompiledSieve>` |
| `evaluate_sieve(compiled, msg)` | Evaluate against a message (no envelope) |
| `evaluate_sieve_with_envelope(compiled, msg, from, to)` | Evaluate with envelope info (needed for `vacation`, `:envelope`) |
| `SieveAction` | Enum of delivery decisions: `Keep`, `FileInto(String)`, `Discard`, `Redirect(String)`, `Reject(String)`, `Vacation(String, Vec<u8>)` |

## What this crate does NOT do

- No **script storage** — caller persists scripts however they like
  (PG, file, ManageSieve).
- No **delivery** — caller's MTA executes the `SieveAction`.
- No **vacation deduplication** — caller tracks "have we replied to
  this address in the last N days?" using their own state store.
- No **`notify` extension** specifics — `notify` reply bodies are
  returned via `SieveAction::Vacation` (same shape, different
  semantics; caller decides whether to actually send).

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-sieve`) |
| **test** | line cov: 96.4% (`cargo llvm-cov -p mailrs-sieve --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 2 gate(s) `perf_gate.rs` |
| **size** | release rlib: 113 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
