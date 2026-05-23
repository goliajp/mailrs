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
//! This 1.0 release implements the structural layer fully and gates
//! the cryptographic layer behind [`verify_chain_with_crypto`]
//! (which returns [`ChainOutcome::CryptoUnimplemented`] for now —
//! 1.1 will plug in the AMS / AS hash + RSA verify, reusing
//! [`mailrs_dkim::canon`] for byte-identical canonicalization).
//!
//! The structural layer alone is enough to:
//!
//! - Detect malformed / sparse / over-long chains (rejecting before
//!   any DNS lookup).
//! - Detect `cv=` inconsistencies that prove the chain was tampered
//!   with (e.g. first set with `cv=pass`, or two sets with `cv=none`).
//! - Carry the chain forward to the cryptographic layer when it
//!   lands in 1.1.

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
    /// implemented (slated for 1.1). Treat as `Pass` for structural
    /// trust, fall back to non-ARC DMARC for the cryptographic call.
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
/// **Not implemented in 1.0.** Returns
/// [`ChainOutcome::CryptoUnimplemented`] after running the structural
/// layer ([`verify_chain`]); if the structural layer fails, that
/// failure is returned instead.
///
/// 1.1 will fill in, for each set from highest `i` downward:
/// compute the AMS body+header hash via [`mailrs_dkim::canon`],
/// DNS-lookup `<selector>._domainkey.<domain>` for both the AMS
/// and AS keys, then RSA-SHA256 or Ed25519-SHA256 verify each
/// signature. Stop on the first failure; the chain's verdict is
/// whatever the highest-instance set's verify yields.
pub async fn verify_chain_with_crypto<R: ArcResolver + ?Sized>(
    chain: &ArcChain,
    _resolver: &R,
    _raw_message: &[u8],
) -> Result<ChainOutcome, ArcError> {
    match verify_chain(chain) {
        ChainOutcome::Pass => Ok(ChainOutcome::CryptoUnimplemented),
        other => Ok(other),
    }
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
    async fn crypto_verify_returns_unimplemented_when_structural_passes() {
        let m = msg_with(&[SET1_AAR, SET1_AMS, SET1_AS_NONE]);
        let chain = ArcChain::extract(&m).unwrap().unwrap();
        let r = verify_chain_with_crypto(&chain, &DummyResolver, &m)
            .await
            .unwrap();
        assert_eq!(r, ChainOutcome::CryptoUnimplemented);
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
