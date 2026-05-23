//! ARC chain verification (RFC 8617 §5).
//!
//! Verification has two layers:
//!
//! 1. **Structural** — every set is complete and contiguous (handled
//!    by [`ArcChain::extract`]); `cv=` values are internally consistent
//!    (first set must be `cv=none`, every later set must be `cv=pass`
//!    or `cv=fail`); chain length ≤ 50.
//! 2. **Cryptographic** — for each instance from highest to lowest:
//!    verify the AMS over the (canonicalized) message + signed
//!    headers, then verify the AS over the (canonicalized) chain
//!    prefix. Both use DKIM's canonicalization rules and the same
//!    signature algorithms (RSA-SHA256 / Ed25519-SHA256) per
//!    RFC 8617 §5.
//!
//! Since 1.1, [`verify_chain_with_crypto`] runs the full crypto layer:
//! it walks the chain from the highest instance down, verifying each
//! AMS + AS against DNS-fetched keys via [`crate::crypto`]. The
//! structural-only variant [`verify_chain`] remains available for
//! callers that want early rejection before any DNS lookup.

use crate::chain::ArcChain;
use crate::error::ArcError;
use crate::header::{ArcSealCv, MAX_INSTANCE};
use crate::resolver::ArcResolver;

/// Outcome of [`verify_chain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainOutcome {
    /// All structural checks pass. The chain is well-formed and the
    /// `cv=` integrity rules hold. Use the chain's accumulated
    /// `Authentication-Results` for downstream DMARC.
    Pass,
    /// The chain has at least one violation. `reason` is a short
    /// human-readable explanation suitable for the `arc=fail reason="…"`
    /// authres field per RFC 8601.
    Fail {
        /// Short fail reason for AuthResults output.
        reason: String,
    },
    /// Structural layer passed; cryptographic layer not yet
    /// implemented. Returned by [`verify_chain_with_crypto`] only when
    /// it is invoked from a path that requested structural-only
    /// (none of the current public APIs return this — it is retained
    /// for compatibility with 1.0 callers that pattern-matched on it).
    #[deprecated(
        since = "1.1.0",
        note = "verify_chain_with_crypto now performs cryptographic verification; \
                this variant is no longer returned"
    )]
    CryptoUnimplemented,
}

/// Verify the structural integrity of an ARC chain.
///
/// Does not perform DNS lookups or signature crypto in 1.0; that's
/// the job of [`verify_chain_with_crypto`] in 1.1. Even just the
/// structural verdict is useful: a chain that violates §5.1
/// integrity rules can be rejected before any crypto work.
pub fn verify_chain(chain: &ArcChain) -> ChainOutcome {
    if chain.sets.is_empty() {
        return ChainOutcome::Fail {
            reason: "empty chain".into(),
        };
    }
    if chain.sets.len() > MAX_INSTANCE as usize {
        return ChainOutcome::Fail {
            reason: format!("chain length {} exceeds RFC 8617 §4.2.1 max of 50", chain.sets.len()),
        };
    }
    // First set must have cv=none; every later set must have cv=pass
    // or cv=fail. RFC 8617 §5.1.
    for (idx, set) in chain.sets.iter().enumerate() {
        if idx == 0 {
            if set.seal.cv != ArcSealCv::None {
                return ChainOutcome::Fail {
                    reason: format!(
                        "first ARC-Seal must have cv=none, got cv={:?}",
                        set.seal.cv
                    ),
                };
            }
        } else if set.seal.cv == ArcSealCv::None {
            return ChainOutcome::Fail {
                reason: format!("ARC-Seal i={} has cv=none but is not the first set", set.i),
            };
        }
    }
    // AMS and AS at every instance must reference an instance that
    // matches the set's i= — already guaranteed by ArcChain::extract,
    // which keys the map on the parsed i=. Double-check for paranoia.
    for (idx, set) in chain.sets.iter().enumerate() {
        let expected = (idx + 1) as u32;
        if set.i != expected || set.aar.instance != expected || set.ams.instance != expected
            || set.seal.instance != expected
        {
            return ChainOutcome::Fail {
                reason: format!("ARC set {idx} has mismatched i= across its 3 headers"),
            };
        }
    }
    ChainOutcome::Pass
}

/// Full ARC chain verification including cryptographic checks.
///
/// For each set from highest instance down, verifies (a) the AMS
/// against the message body + signed headers, and (b) the AS against
/// the chain prefix per RFC 8617 §5.1.2. A single failure returns
/// [`ChainOutcome::Fail`] with a short reason; the chain only
/// achieves [`ChainOutcome::Pass`] when every AMS and every AS
/// verifies cryptographically.
///
/// Crypto delegates to [`crate::crypto`] which re-uses
/// [`mailrs_dkim::crypto`] for the actual RSA-SHA256 /
/// Ed25519-SHA256 primitive.
pub async fn verify_chain_with_crypto<R: ArcResolver + ?Sized>(
    chain: &ArcChain,
    resolver: &R,
    raw_message: &[u8],
) -> Result<ChainOutcome, ArcError> {
    match verify_chain(chain) {
        ChainOutcome::Pass => {}
        other => return Ok(other),
    }
    // Walk highest → lowest; a tampered later set is the most useful
    // signal for downstream DMARC (the latest forwarder is who
    // attaches the chain we trust). For each instance verify AMS then
    // AS — both must pass.
    for set in chain.sets.iter().rev() {
        if let Err(e) = crate::crypto::verify_ams(set, raw_message, resolver).await {
            return Ok(ChainOutcome::Fail {
                reason: format!("ams i={}: {e}", set.i),
            });
        }
        if let Err(e) = crate::crypto::verify_as(chain, set.i, resolver).await {
            return Ok(ChainOutcome::Fail {
                reason: format!("as i={}: {e}", set.i),
            });
        }
    }
    Ok(ChainOutcome::Pass)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::ArcChain;

    fn msg_with(headers: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for h in headers {
            v.extend_from_slice(h.as_bytes());
        }
        v.extend_from_slice(b"From: a@b.c\r\nSubject: t\r\n\r\nbody");
        v
    }

    const SET1_AAR: &str = "ARC-Authentication-Results: i=1; spf=pass\r\n";
    const SET1_AMS: &str = "ARC-Message-Signature: i=1; a=rsa-sha256; d=example.com; s=mail; h=From; bh=BH1; b=SIG1\r\n";
    const SET1_AS_NONE: &str = "ARC-Seal: i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; b=SEAL1\r\n";
    const SET1_AS_PASS: &str = "ARC-Seal: i=1; a=rsa-sha256; cv=pass; d=example.com; s=mail; b=SEAL1\r\n";

    const SET2_AAR: &str = "ARC-Authentication-Results: i=2; dkim=pass\r\n";
    const SET2_AMS: &str = "ARC-Message-Signature: i=2; a=rsa-sha256; d=forwarder.example; s=mail; h=From; bh=BH2; b=SIG2\r\n";
    const SET2_AS_PASS: &str = "ARC-Seal: i=2; a=rsa-sha256; cv=pass; d=forwarder.example; s=mail; b=SEAL2\r\n";
    const SET2_AS_NONE: &str = "ARC-Seal: i=2; a=rsa-sha256; cv=none; d=forwarder.example; s=mail; b=SEAL2\r\n";

    #[test]
    fn single_set_with_cv_none_passes_structural() {
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_NONE]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        assert_eq!(verify_chain(&chain), ChainOutcome::Pass);
    }

    #[test]
    fn single_set_with_cv_pass_fails_structural() {
        // First set must be cv=none.
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_PASS]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        assert!(matches!(verify_chain(&chain), ChainOutcome::Fail { .. }));
    }

    #[test]
    fn two_set_normal_chain_passes_structural() {
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_NONE, SET2_AAR, SET2_AMS, SET2_AS_PASS]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        assert_eq!(verify_chain(&chain), ChainOutcome::Pass);
    }

    #[test]
    fn later_set_with_cv_none_fails_structural() {
        // Second set with cv=none is illegal.
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_NONE, SET2_AAR, SET2_AMS, SET2_AS_NONE]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        assert!(matches!(verify_chain(&chain), ChainOutcome::Fail { .. }));
    }

    struct DummyResolver;
    #[async_trait::async_trait]
    impl mailrs_dkim::DkimResolver for DummyResolver {
        async fn lookup_txt(&self, _: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn crypto_verify_returns_fail_when_dns_empty_after_structural_pass() {
        // Structural layer passes, but the DummyResolver returns an
        // empty TXT vector — fetch_public_key bubbles that up as Dns,
        // which verify_chain_with_crypto maps to ChainOutcome::Fail
        // with a "ams i=1: …" reason.
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_NONE]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        let r = verify_chain_with_crypto(&chain, &DummyResolver, &m)
            .await
            .unwrap();
        match r {
            ChainOutcome::Fail { reason } => assert!(reason.starts_with("ams i=1:"), "{reason}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn crypto_verify_returns_fail_when_structural_fails() {
        // First-set cv=pass — fails structural; verify_chain_with_crypto
        // must propagate that.
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_PASS]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        let r = verify_chain_with_crypto(&chain, &DummyResolver, &m)
            .await
            .unwrap();
        assert!(matches!(r, ChainOutcome::Fail { .. }));
    }
}
