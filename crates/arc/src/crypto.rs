//! ARC cryptographic verification (RFC 8617 §5).
//!
//! Two distinct signature paths share the same RSA-SHA256 /
//! Ed25519-SHA256 primitive (re-exposed by `mailrs_dkim::crypto`):
//!
//! - **AMS verify** ([`verify_ams`]) — the per-hop `ARC-Message-Signature`
//!   signs the body (canonicalized with `c=` body alg, hashed → `bh=`)
//!   plus the listed `h=` headers (canonicalized with `c=` header alg)
//!   plus the AMS header itself with `b=` cleared. Functionally identical
//!   to RFC 6376 §3.7 DKIM verification, except the signature header
//!   name is `ARC-Message-Signature` and there can be multiple of them
//!   (one per instance) in the same message.
//!
//! - **AS verify** ([`verify_as`]) — `ARC-Seal` signs the chain
//!   prefix. Per §5.1.2 the input is the concatenation of the
//!   canonicalized `ARC-Authentication-Results` / `ARC-Message-Signature`
//!   / `ARC-Seal` triples for instances `1..i`, each in that order, plus
//!   the current `ARC-Seal` (i = N) with its `b=` cleared. Always
//!   relaxed/relaxed canonicalization regardless of any `c=` tag.

use base64::Engine as _;
use sha2::{Digest, Sha256};

use mailrs_dkim::canon::{canonicalize_body, canonicalize_header};
use mailrs_dkim::crypto::{extract_public_key, verify_signature};
use mailrs_dkim::header::{Algorithm as DkimAlgorithm, Canon as DkimCanon};
use mailrs_dkim::headers::{
    body_offset_minus_blank, clear_b_value, find_body_offset, find_header_value,
};

use crate::chain::{ArcChain, ArcSet};
use crate::error::ArcError;
use crate::header::{Algorithm, Canon};
use crate::resolver::ArcResolver;

fn map_algorithm(a: Algorithm) -> DkimAlgorithm {
    match a {
        Algorithm::RsaSha256 => DkimAlgorithm::RsaSha256,
        Algorithm::Ed25519Sha256 => DkimAlgorithm::Ed25519Sha256,
    }
}

fn map_canon(c: Canon) -> DkimCanon {
    match c {
        Canon::Simple => DkimCanon::Simple,
        Canon::Relaxed => DkimCanon::Relaxed,
    }
}

/// DNS-fetch the `<selector>._domainkey.<domain>` TXT and parse the
/// `p=` payload into raw public-key bytes (PKCS8 DER for RSA, raw
/// 32 bytes for Ed25519). Same shape `mailrs-dkim` uses.
async fn fetch_public_key<R: ArcResolver + ?Sized>(
    resolver: &R,
    selector: &str,
    domain: &str,
) -> Result<Vec<u8>, ArcError> {
    let q = format!("{selector}._domainkey.{domain}");
    let txts = resolver
        .lookup_txt(&q)
        .await
        .map_err(|e| ArcError::Dns(format!("{q}: {e}")))?;
    if txts.is_empty() {
        return Err(ArcError::Dns(format!("no TXT at {q}")));
    }
    let key_txt = txts
        .iter()
        .find(|s| s.contains("p="))
        .ok_or_else(|| ArcError::InvalidPublicKey(format!("no p= tag in TXT at {q}")))?;
    extract_public_key(key_txt).map_err(|e| ArcError::InvalidPublicKey(format!("{q}: {e}")))
}

/// Verify the ARC-Message-Signature of one set against the raw
/// message. Returns `Ok(())` on success or a specific `ArcError` on
/// failure.
///
/// **Process** (RFC 8617 §5.1.1 → DKIM §3.7):
///
/// 1. Find body offset, canonicalize body with `c=` body alg, SHA-256,
///    compare to `bh=`.
/// 2. For each header listed in `h=`: canonicalize with `c=` header
///    alg, append to signed-block.
/// 3. Canonicalize the AMS header itself (name = `ARC-Message-Signature`)
///    with `b=` value cleared, strip trailing CRLF, append.
///
/// AMS lookups must use the **specific instance**'s AMS header — there
/// is generally more than one in the message (one per hop). The
/// caller passes the chain set so we use `set.raw_ams` (which is the
/// exact original value for this instance).
pub async fn verify_ams<R: ArcResolver + ?Sized>(
    set: &ArcSet,
    raw_message: &[u8],
    resolver: &R,
) -> Result<(), ArcError> {
    let ams = &set.ams;

    // 1. body-hash
    let body_offset = find_body_offset(raw_message).ok_or(ArcError::MalformedMessage)?;
    let body = &raw_message[body_offset..];
    let canon_body_bytes = canonicalize_body(body, map_canon(ams.canon_body), None);
    let mut body_hasher = Sha256::new();
    body_hasher.update(&canon_body_bytes);
    let actual_body_hash = body_hasher.finalize();
    let expected_body_hash = base64::engine::general_purpose::STANDARD
        .decode(&ams.body_hash_b64)
        .map_err(|_| ArcError::InvalidBase64("bh".into()))?;
    if actual_body_hash.as_slice() != expected_body_hash.as_slice() {
        return Err(ArcError::BodyHashMismatch);
    }

    // 2. fetch public key
    let key_bytes = fetch_public_key(resolver, &ams.selector, &ams.domain).await?;

    // 3. signed-block = canon(h=…) ++ canon(AMS-with-b=empty)
    let headers_region = &raw_message[..body_offset_minus_blank(body_offset, raw_message)];
    let mut signed_block = Vec::with_capacity(512);
    for name in &ams.signed_headers {
        if let Some(value) = find_header_value(headers_region, name) {
            let canon = canonicalize_header(name, value, map_canon(ams.canon_header));
            signed_block.extend_from_slice(&canon);
        }
    }
    // Append the AMS header itself (this instance's raw value), b= cleared.
    let ams_cleared = clear_b_value(&set.raw_ams);
    let canon_ams = canonicalize_header(
        "ARC-Message-Signature",
        &ams_cleared,
        map_canon(ams.canon_header),
    );
    let canon_ams_trimmed = if canon_ams.ends_with(b"\r\n") {
        &canon_ams[..canon_ams.len() - 2]
    } else {
        &canon_ams
    };
    signed_block.extend_from_slice(canon_ams_trimmed);

    // 4. signature verify
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(&ams.signature_b64)
        .map_err(|_| ArcError::InvalidBase64("b".into()))?;
    verify_signature(
        map_algorithm(ams.algorithm),
        &key_bytes,
        &signed_block,
        &signature_bytes,
    )
    .map_err(|_| ArcError::SignatureMismatch {
        header: "ARC-Message-Signature",
        instance: set.i,
    })?;
    Ok(())
}

/// Verify one ARC-Seal against the chain prefix (instances 1..=i),
/// per RFC 8617 §5.1.2.
///
/// Input to hash (always relaxed/relaxed canon):
/// - For each instance `j` in `1..i`: canon(AAR_j) ++ canon(AMS_j) ++ canon(AS_j)
///   (each with its own `b=` intact, in the order they appear above)
/// - Finally: canon(AS_i with b=cleared, trailing CRLF stripped)
///
/// Note that AS at instance `i` is the only seal whose `b=` is
/// cleared; prior seals contribute their full signed value.
pub async fn verify_as<R: ArcResolver + ?Sized>(
    chain: &ArcChain,
    instance: u32,
    resolver: &R,
) -> Result<(), ArcError> {
    let idx = (instance as usize)
        .checked_sub(1)
        .ok_or(ArcError::MalformedMessage)?;
    let current = chain.sets.get(idx).ok_or(ArcError::MalformedMessage)?;
    let canon = DkimCanon::Relaxed;

    let mut signed_block = Vec::with_capacity(512);
    for j in 0..idx {
        let prior = &chain.sets[j];
        signed_block.extend_from_slice(&canonicalize_header(
            "ARC-Authentication-Results",
            &prior.raw_aar,
            canon,
        ));
        signed_block.extend_from_slice(&canonicalize_header(
            "ARC-Message-Signature",
            &prior.raw_ams,
            canon,
        ));
        signed_block.extend_from_slice(&canonicalize_header("ARC-Seal", &prior.raw_seal, canon));
    }
    // Current set: AAR + AMS contribute full values, AS has b= cleared.
    signed_block.extend_from_slice(&canonicalize_header(
        "ARC-Authentication-Results",
        &current.raw_aar,
        canon,
    ));
    signed_block.extend_from_slice(&canonicalize_header(
        "ARC-Message-Signature",
        &current.raw_ams,
        canon,
    ));
    let seal_cleared = clear_b_value(&current.raw_seal);
    let canon_seal = canonicalize_header("ARC-Seal", &seal_cleared, canon);
    let canon_seal_trimmed = if canon_seal.ends_with(b"\r\n") {
        &canon_seal[..canon_seal.len() - 2]
    } else {
        &canon_seal
    };
    signed_block.extend_from_slice(canon_seal_trimmed);

    // Crypto.
    let seal = &current.seal;
    let key_bytes = fetch_public_key(resolver, &seal.selector, &seal.domain).await?;
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(&seal.signature_b64)
        .map_err(|_| ArcError::InvalidBase64("b".into()))?;
    verify_signature(
        map_algorithm(seal.algorithm),
        &key_bytes,
        &signed_block,
        &signature_bytes,
    )
    .map_err(|_| ArcError::SignatureMismatch {
        header: "ARC-Seal",
        instance,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The honest end-to-end crypto tests require real RSA/Ed25519 key
    // pairs + signing logic. These live in `tests/crypto_roundtrip.rs`
    // (an integration test) because they also need an `ArcResolver`
    // impl that returns the public key. The unit tests here only
    // exercise the easy negative-path branches that don't need crypto.

    #[test]
    fn map_algorithm_roundtrip() {
        assert_eq!(
            map_algorithm(Algorithm::RsaSha256),
            DkimAlgorithm::RsaSha256
        );
        assert_eq!(
            map_algorithm(Algorithm::Ed25519Sha256),
            DkimAlgorithm::Ed25519Sha256
        );
    }

    #[test]
    fn map_canon_roundtrip() {
        assert_eq!(map_canon(Canon::Simple), DkimCanon::Simple);
        assert_eq!(map_canon(Canon::Relaxed), DkimCanon::Relaxed);
    }
}
