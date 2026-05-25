# mailrs-srs

[![Crates.io](https://img.shields.io/crates/v/mailrs-srs?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-srs)
[![docs.rs](https://img.shields.io/docsrs/mailrs-srs?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-srs)
[![License](https://img.shields.io/crates/l/mailrs-srs?style=flat-square)](#license)

Sender Rewriting Scheme (SRS) primitive for SPF-aware mail forwarding.

When a forwarder relays mail with the original envelope-From intact,
the receiver's SPF check sees the forwarder's IP against the original
sender's domain — and rejects. SRS rewrites the envelope-From into a
form that:

1. Identifies the **forwarder** as the responsible sender (passes SPF)
2. Carries the **original** sender encoded into the local-part, so
   bounces can be reverse-mapped back to the original

`mailrs-srs` provides both directions: forward `rewrite()` for the
outgoing relay, reverse `reverse()` for incoming bounces with full
HMAC verification + timestamp-window check.

## Quickstart

```rust
use mailrs_srs::{rewrite, reverse, DEFAULT_TIMESTAMP_WINDOW_DAYS};

// Forward: rewrite envelope-From when relaying a message.
let original = "alice@example.com";
let forwarder_domain = "mx.golia.jp";
let secret = "shared-secret-32-bytes-or-whatever";

let rewritten = rewrite(original, forwarder_domain, secret);
//  →  "SRS0=ab12cd34=072=example.com=alice@mx.golia.jp"

// Reverse: when a bounce comes back to the SRS-rewritten address,
// verify the HMAC + parse the original sender.
let bounce_target = &rewritten;
let recovered = reverse(bounce_target, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
assert_eq!(recovered.as_deref(), Some("alice@example.com"));

// Wrong secret: rejects.
assert!(reverse(bounce_target, "wrong-secret", 14).is_none());
// Tampered hash: rejects.
let tampered = bounce_target.replace("SRS0=", "SRS1=");
assert!(reverse(&tampered, secret, 14).is_none());
```

## Wire format

```text
SRS0=<hash>=<tt>=<original-domain>=<local-part>@<local-domain>
        │     │           │             │            │
        │     │           │             │            └─ forwarder's own domain
        │     │           │             └─ original sender's local-part
        │     │           └─ original sender's domain
        │     └─ timestamp: days_since_epoch mod 1024, 3-digit zero-padded
        └─ HMAC-SHA256 truncated to 8 hex chars (32 bits)
```

The 32-bit truncated HMAC is enough to prevent online guessing (one
attempt per network round trip) given a per-day timestamp. The
timestamp window defaults to 14 days; bounces older than that fail
verification, which keeps stale bounce traffic from chewing through
attacker probe attempts.

`SRS1` (forwarding-chain) is not implemented in 1.0; it's only needed
when a forwarder receives mail that was *already* SRS-rewritten by
another forwarder (multi-hop forwarding). Most deployments don't see
this; if you need it, file an issue.

## Constant-time HMAC comparison

`reverse()` uses constant-time byte comparison on the HMAC bytes so an
attacker who can call your reverse-lookup endpoint in a loop can't
recover the secret by timing-side-channel. (The HMAC value itself is
not secret — the *key* used to compute it is. The constant-time check
prevents byte-by-byte secret recovery.)

## What this crate is not

- **Not** a full SMTP relay implementation. Plug `rewrite()` /
  `reverse()` into your existing relay's envelope-handling.
- **Not** an SPF verifier. Use `mail-auth` (or whatever crate you
  have) for the actual SPF lookup; SRS only solves the
  "forwarder's IP fails the original sender's SPF" problem.
- **Not** a DKIM rewriter. DKIM signatures cover the message body +
  selected headers, not the envelope, so DKIM forwarding is unrelated
  to SRS.

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `rewrite` (ASCII sender) | ~270 ns |
| `reverse` (success path, in-window) | ~290 ns |
| `reverse` (wrong-secret rejection) | ~280 ns (constant-time) |

Run: `cargo bench -p mailrs-srs --bench srs`. Reproduce numbers in
[BUDGETS.md](BUDGETS.md).

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-srs`) |
| **test** | line cov: 99.1% (`cargo llvm-cov -p mailrs-srs --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 4 gate(s) `perf_gate.rs` |
| **size** | release rlib: 54 KB |
| **fuzz** | ✅ 1 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
