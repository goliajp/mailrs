# mailrs-arc

RFC 8617 — **Authenticated Received Chain (ARC)** header parsing,
chain extraction, and structural verification.

Part of the [mailrs](https://github.com/goliajp/mailrs) "stones" family.
Pairs with [`mailrs-dkim`](https://crates.io/crates/mailrs-dkim) for
canonicalization + RSA verify and with
[`mailrs-spf`](https://crates.io/crates/mailrs-spf) /
[`mailrs-dmarc`](https://crates.io/crates/mailrs-dmarc) for the
authentication results that ARC carries forward.

## What ARC is

ARC fixes the longstanding "forwarders break DMARC" problem. Every
forwarding hop adds a triplet of headers indexed by `i=N`:

```
ARC-Authentication-Results: i=N; <authres-body>
ARC-Message-Signature:      i=N; <dkim-like-signature>
ARC-Seal:                   i=N; cv={none|pass|fail}; <signature-over-chain>
```

A downstream verifier walks the chain from `i=1` upward, validates each
set, and produces a verdict (`pass` / `fail`) that DMARC can use to
override forwarder breakage.

## What this crate covers (1.0)

| Layer | Status |
|---|---|
| Parse `ARC-Authentication-Results` / `ARC-Message-Signature` / `ARC-Seal` | ✅ |
| Extract chain from raw message + group by instance | ✅ |
| Validate chain contiguity (no gaps from `i=1`) | ✅ |
| Validate `cv=` integrity (first = `none`, rest = `pass`/`fail`) | ✅ |
| Cryptographic AMS + AS verify (RSA-SHA256 / Ed25519-SHA256) | 1.1 — see below |
| ARC sealing (add a new set on outbound forward) | 1.1 |

The structural layer alone is enough to:

- Detect malformed / sparse / over-long chains (rejecting before
  any DNS work).
- Detect `cv=` inconsistencies that prove the chain was tampered with
  (first set with `cv=pass`, two sets with `cv=none`, etc.).
- Carry the chain forward to the cryptographic layer in 1.1.

Crypto in 1.1 will reuse [`mailrs_dkim::canon`] byte-for-byte —
RFC 8617 §5 says ARC-Message-Signature uses the same algorithms and
canonicalization as DKIM-Signature, so we route through the
battle-tested implementation instead of duplicating ~400 LOC.

## Example

```rust
use mailrs_arc::{ArcChain, verify_chain, ChainOutcome};

let raw_message: &[u8] = b"\
ARC-Authentication-Results: i=1; spf=pass smtp.mailfrom=alice@example.com\r\n\
ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail; h=From:To:Subject; bh=BH1; b=SIG1\r\n\
ARC-Seal: i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; b=SEAL1\r\n\
From: alice@example.com\r\n\
Subject: hi\r\n\
\r\n\
body";

let chain = ArcChain::extract(raw_message).unwrap().unwrap();
assert_eq!(chain.sets.len(), 1);
assert_eq!(verify_chain(&chain), ChainOutcome::Pass);
```

## Performance

Measured (criterion, M-series Mac, release):

| Operation | Median |
|---|---:|
| `ArcAuthResults::parse` (1 set) | **21 ns** |
| `ArcMessageSignature::parse` (realistic) | **479 ns** |
| `ArcSeal::parse` (realistic) | **295 ns** |
| `ArcChain::extract` (2-hop chain) | **3.65 µs** |

Reproduce: `cargo bench -p mailrs-arc --bench arc`.

## Why this crate exists

Before mailrs-arc 1.0, the only Rust ARC implementation was the
[`mail-auth`](https://crates.io/crates/mail-auth) umbrella, which
bundles SPF/DKIM/DMARC/ARC into a single ~5K-LOC crate. Picking it up
for "just ARC" pulls in everything else.

mailrs-arc 1.0 ships ARC as a standalone primitive. Use it with
mailrs-spf / mailrs-dkim / mailrs-dmarc (the rest of the email-auth
stack) or stand-alone with whatever auth stack you already have.

For mailrs's own server, mailrs-arc 1.0 closes
[DEPS_AUDIT](https://github.com/goliajp/mailrs/blob/main/DEPS_AUDIT.md)
candidate #1 — the server can drop `mail-auth` from its runtime
dependencies once 1.1 ships the cryptographic layer.

## License

Apache-2.0 OR MIT.
