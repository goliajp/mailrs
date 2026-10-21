# mailrs-webhook-signature

[![Crates.io](https://img.shields.io/crates/v/mailrs-webhook-signature?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-webhook-signature)
[![docs.rs](https://img.shields.io/docsrs/mailrs-webhook-signature?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-webhook-signature)
[![License](https://img.shields.io/crates/l/mailrs-webhook-signature?style=flat-square)](#license)

HMAC-SHA256 webhook payload signing + verification. The exact pattern
that GitHub / Stripe / Linear / Shopify all use:

```text
X-Webhook-Signature: sha256=<HMAC-SHA256(secret, payload)>
```

**Constant-time HMAC compare** (`hmac::Mac::verify_slice` does this
under the hood). **Optional secret rotation** via `verify_any`.

## Quickstart

```rust
use mailrs_webhook_signature::{sign, verify_header, format_header};

// === sender side ===
let secret = b"shared-32-byte-secret-here";
let payload = br#"{"event":"new_message","id":42}"#;
let sig = sign(secret, payload);
let header = format_header(&sig);
// POST with header X-Webhook-Signature: header

// === receiver side ===
// (header_value pulled from request)
let header_value = header.clone();
let payload_received = payload.to_vec();
assert!(verify_header(secret, &payload_received, &header_value));

// Tampered payload? rejected.
let tampered = b"{}";
assert!(!verify_header(secret, tampered, &header_value));
```

## Secret rotation

When rolling the signing secret, the receiver knows both keys
during a transition window. Use `verify_any`:

```rust
use mailrs_webhook_signature::{sign, verify_any};

// Some webhook was signed with the OLD secret (sender hasn't
// rolled yet).
let sig = sign(b"old-secret", b"payload");

// Receiver knows both — verification succeeds:
assert!(verify_any(&[b"new-secret", b"old-secret"], b"payload", &sig));
```

After all senders have rolled, drop `old-secret` from the list.

## What this crate does

- **sign(secret, payload)** — HMAC-SHA256, returns 64-char lowercase hex
- **verify(secret, payload, signature)** — constant-time HMAC compare
- **format_header(signature)** — `"sha256=<hex>"` shape
- **parse_header(header_value)** — strips `sha256=` prefix if present,
  trims whitespace
- **verify_header(secret, payload, header_value)** — sugar wrapper
- **verify_any(&[secret1, secret2, …], payload, signature)** —
  rotation support

That's the whole surface. ~10 lines of API.

## What this crate does not

- **No timestamp tolerance / replay protection.** GitHub-style
  signature schemes often include a timestamp in the signed payload;
  this crate signs the **raw payload bytes** only. If you need replay
  protection, embed your own timestamp inside the payload before
  signing.
- **No algorithm negotiation.** SHA-256 only. Most webhook APIs in
  production use exactly this; if you need SHA-512 or BLAKE3,
  write a tiny crate for that.
- **No async / I/O.** Sign + verify are pure byte ops. Bring your
  own HTTP client.

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `sign` (32-byte payload) | **~420 ns** |
| `sign` (1 KB payload) | **~1.6 µs** |
| `sign` (100 KB payload) | **~92 µs** |
| `verify` (correct, 32-byte) | **~690 ns** |
| `verify` (wrong secret, constant-time) | **~650 ns** |
| `verify_any` (2 secrets, first matches) | **~700 ns** |
| `verify_any` (2 secrets, second matches) | **~915 ns** |
| `format_header` | **~36 ns** |
| `parse_header` (with prefix) | **~16 ns** |

Reproduce: `cargo bench -p mailrs-webhook-signature --bench signing`.
Workspace [PERFORMANCE.md](../../PERFORMANCE.md) carries the same
table.

## Security notes

- **Use a 256-bit (32-byte) random secret minimum.** `HMAC-SHA256`
  keys can be any length, but shorter keys reduce the brute-force
  search space.
- **Generate secrets with a cryptographically secure RNG.** Use
  `rand::rngs::OsRng` or equivalent; don't use `rand::thread_rng()`
  alone (it's secure on most platforms but the contract is weaker).
- **Compare with this crate's `verify`/`verify_header`.** Do not
  hand-roll byte comparison — `==` on `&[u8]` is NOT constant-time
  and leaks key material via timing.

## License

Apache-2.0 OR MIT.
