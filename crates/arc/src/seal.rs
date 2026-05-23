//! ARC sealing (RFC 8617 §5.1) — outbound forwarder builds and
//! attaches the three headers that prove this hop's view of the
//! chain.
//!
//! Given a raw inbound message + the prior chain (or none, if this
//! is the first hop) + a signing key + an Authentication-Results
//! body to attach as this hop's AAR, [`seal`] produces three header
//! lines (`AAR`, `AMS`, `AS`) ready to prepend in that order to the
//! outbound wire bytes.
//!
//! ## What sealing actually does (the §5.1 walk)
//!
//! 1. **Determine `i=N`.** The new instance number is `prior.highest_instance()
//!    + 1`, or `1` if there's no prior chain.
//! 2. **Build AAR** at `i=N`. Just `i={N}; {authres}` — not
//!    cryptographically signed by itself; pulled into the AS hash later.
//! 3. **Build AMS** at `i=N`. DKIM-Signature-shaped: body hash via
//!    canon_body, signed header block via canon_header over the listed
//!    `h=` headers + the AMS itself with `b=` empty.
//! 4. **Build AS** at `i=N`. Always relaxed/relaxed canon (RFC 8617
//!    §5.1.1). Signed input: the prior chain's canonicalized
//!    `(AAR, AMS, AS)` triples for `j=1..N-1` in instance order, then
//!    the new `(AAR, AMS)` at `i=N`, then the new AS with `b=` empty.
//! 5. The `cv=` value on the new seal: `none` when `N=1` (no prior),
//!    `pass` or `fail` per the caller's verdict on the prior chain.
//!
//! The output is byte-identical to what `mailrs_arc::verify_chain_with_crypto`
//! expects on the verify side — that's the test contract.

use base64::Engine as _;
use sha2::{Digest, Sha256};

use mailrs_dkim::canon::{canonicalize_body, canonicalize_header};
use mailrs_dkim::crypto::{CryptoSigningKey, sign_signature};
use mailrs_dkim::header::Canon as DkimCanon;
use mailrs_dkim::headers::{
    body_offset_minus_blank, clear_b_value, find_body_offset, find_header_value,
};

use crate::chain::ArcChain;
use crate::error::ArcError;
use crate::header::{Algorithm, ArcSealCv, Canon, MAX_INSTANCE};

/// Private key used to sign ARC AMS + AS headers. Mirrors the same
/// shape as `mailrs_dkim::DkimSigningKey`. Algorithm is implied.
pub enum ArcSigningKey<'a> {
    /// RSA → `a=rsa-sha256` on both AMS and AS.
    Rsa(&'a rsa::RsaPrivateKey),
    /// Ed25519 → `a=ed25519-sha256` on both AMS and AS (RFC 8463).
    Ed25519(&'a ed25519_dalek::SigningKey),
}

impl<'a> ArcSigningKey<'a> {
    fn algorithm(&self) -> Algorithm {
        match self {
            Self::Rsa(_) => Algorithm::RsaSha256,
            Self::Ed25519(_) => Algorithm::Ed25519Sha256,
        }
    }

    fn as_crypto(&self) -> CryptoSigningKey<'_> {
        match self {
            Self::Rsa(k) => CryptoSigningKey::Rsa(k),
            Self::Ed25519(k) => CryptoSigningKey::Ed25519(k),
        }
    }
}

/// Options controlling what the forwarder seals.
#[derive(Debug, Clone)]
pub struct SealOpts {
    /// `d=` — signing domain (this hop's domain).
    pub domain: String,
    /// `s=` — selector (publishes the public key at
    /// `<selector>._domainkey.<domain>`).
    pub selector: String,
    /// Header NAMES to include in the AMS's `h=` list.
    /// Case-insensitive lookup against the message.
    pub signed_headers: Vec<String>,
    /// AMS header canonicalization (RFC 6376 §3.4.1 / §3.4.2 rules).
    pub canon_header: Canon,
    /// AMS body canonicalization.
    pub canon_body: Canon,
    /// `cv=` value to embed in the new AS. Caller passes this based on
    /// its own verification of the prior chain (typically via
    /// [`crate::verify_chain_with_crypto`]):
    /// - `None` when there is no prior chain (first hop, `i=1`).
    /// - `Pass` when the prior chain crypto-verified.
    /// - `Fail` when any prior signature failed to verify.
    pub cv: ArcSealCv,
    /// Verbatim `Authentication-Results` body for this hop, embedded
    /// in the new AAR after the `i=N;` prefix. Same syntax as
    /// RFC 8601's `Authentication-Results` header value.
    pub authres: String,
    /// Optional `t=` timestamp (epoch seconds) on the AMS + AS.
    pub timestamp: Option<u64>,
}

/// Output of [`seal`] — three header lines, in the order they should
/// be prepended to the outbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedHeaders {
    /// `ARC-Authentication-Results: i=N; <authres>\r\n`
    pub aar: String,
    /// `ARC-Message-Signature: i=N; ...\r\n`
    pub ams: String,
    /// `ARC-Seal: i=N; ...\r\n`
    pub seal: String,
}

impl SealedHeaders {
    /// Convenience: all three headers concatenated in AAR, AMS, AS
    /// order, ready to prepend to the message's wire bytes.
    ///
    /// This is the natural order a downstream verifier sees, but the
    /// spec doesn't actually mandate ordering (verify groups by `i=`).
    /// Using a consistent order makes logs and diffs readable.
    pub fn concat(&self) -> String {
        format!("{}{}{}", self.aar, self.ams, self.seal)
    }
}

fn map_canon(c: Canon) -> DkimCanon {
    match c {
        Canon::Simple => DkimCanon::Simple,
        Canon::Relaxed => DkimCanon::Relaxed,
    }
}

fn alg_str(a: Algorithm) -> &'static str {
    match a {
        Algorithm::RsaSha256 => "rsa-sha256",
        Algorithm::Ed25519Sha256 => "ed25519-sha256",
    }
}

fn canon_pair_str(h: Canon, b: Canon) -> &'static str {
    match (h, b) {
        (Canon::Simple, Canon::Simple) => "simple/simple",
        (Canon::Simple, Canon::Relaxed) => "simple/relaxed",
        (Canon::Relaxed, Canon::Simple) => "relaxed/simple",
        (Canon::Relaxed, Canon::Relaxed) => "relaxed/relaxed",
    }
}

fn cv_str(cv: ArcSealCv) -> &'static str {
    match cv {
        ArcSealCv::None => "none",
        ArcSealCv::Pass => "pass",
        ArcSealCv::Fail => "fail",
    }
}

/// Seal a message — produce the three ARC headers a forwarder
/// attaches when relaying.
///
/// `prior` is the chain extracted from the inbound message (via
/// [`ArcChain::extract`]); pass `None` if there is no prior chain
/// (first hop, `opts.cv` must then be [`ArcSealCv::None`]).
///
/// Returns [`SealedHeaders`] containing the three header lines.
/// Prepend them in `aar, ams, seal` order to the message's wire
/// bytes — that's what `verify_chain_with_crypto` expects.
///
/// # Errors
///
/// - [`ArcError::MalformedMessage`] when `raw_message` has no
///   end-of-headers terminator.
/// - [`ArcError::ChainTooLong`] when the new instance would exceed
///   the RFC 8617 §4.2.1 limit of 50.
/// - [`ArcError::InvalidKey`] (rare) when the underlying crypto
///   primitive rejects signing.
pub fn seal(
    raw_message: &[u8],
    key: &ArcSigningKey<'_>,
    opts: &SealOpts,
    prior: Option<&ArcChain>,
) -> Result<SealedHeaders, ArcError> {
    // 1. Determine new instance number.
    let prior_height = prior.map(|c| c.highest_instance()).unwrap_or(0);
    let new_i = prior_height + 1;
    if new_i > MAX_INSTANCE {
        return Err(ArcError::ChainTooLong(new_i as usize));
    }
    // First-hop integrity: cv must be None when no prior, must NOT
    // be None when there's a prior (RFC 8617 §5.1 integrity).
    if prior_height == 0 && opts.cv != ArcSealCv::None {
        return Err(ArcError::InvalidCv(format!(
            "first hop must use cv=none, got cv={}",
            cv_str(opts.cv)
        )));
    }
    if prior_height > 0 && opts.cv == ArcSealCv::None {
        return Err(ArcError::InvalidCv(format!(
            "hop i={new_i} cannot use cv=none (prior chain present)"
        )));
    }

    // 2. Build AAR — just i= and the verbatim authres body.
    // Trim trailing whitespace + CRLF on the authres so the line is
    // single-line. Embedding multi-line authres would require folding
    // and is fragile to author; let the caller flatten.
    let authres = opts.authres.trim_end_matches(['\r', '\n']).trim();
    let aar_value = format!("i={new_i}; {authres}");
    let aar_line = format!("ARC-Authentication-Results: {aar_value}\r\n");

    // 3. Build AMS — DKIM-Signature-shaped at i=N.
    let body_offset = find_body_offset(raw_message).ok_or(ArcError::MalformedMessage)?;
    let body = &raw_message[body_offset..];
    let canon_body_bytes = canonicalize_body(body, map_canon(opts.canon_body), None);
    let mut body_hasher = Sha256::new();
    body_hasher.update(&canon_body_bytes);
    let bh = base64::engine::general_purpose::STANDARD.encode(body_hasher.finalize());

    let alg = alg_str(key.algorithm());
    let canon_str = canon_pair_str(opts.canon_header, opts.canon_body);
    let h_list = opts.signed_headers.join(":");

    // AMS tag list. `b=` MUST be the last tag so appending the
    // signature bytes can't disturb any other tag's value (same
    // invariant as DKIM signing).
    let mut ams_tags = format!(
        "i={new_i}; a={alg}; c={canon_str}; d={d}; s={s}; h={h}; bh={bh}",
        d = opts.domain,
        s = opts.selector,
        h = h_list,
    );
    if let Some(t) = opts.timestamp {
        ams_tags = format!("{ams_tags}; t={t}");
    }
    ams_tags = format!("{ams_tags}; b=");

    // AMS signed-header block (per RFC 6376 §3.7 + 8617 §5.1.1):
    // canon(each h=) + canon(this AMS with b= empty, trailing CRLF stripped)
    let headers_region = &raw_message[..body_offset_minus_blank(body_offset, raw_message)];
    let mut ams_signed_block = Vec::with_capacity(512);
    for name in &opts.signed_headers {
        if let Some(value) = find_header_value(headers_region, name) {
            let canon_name = name.to_ascii_lowercase();
            ams_signed_block.extend_from_slice(&canonicalize_header(
                &canon_name,
                value,
                map_canon(opts.canon_header),
            ));
        }
    }
    // Mirror what verify will see on the wire: leading space after
    // the colon (no trailing \r — clear_b_value will consume it).
    let ams_signed_value = format!(" {ams_tags}");
    let canon_ams = canonicalize_header(
        "ARC-Message-Signature",
        &ams_signed_value,
        map_canon(opts.canon_header),
    );
    let canon_ams_trimmed = if canon_ams.ends_with(b"\r\n") {
        &canon_ams[..canon_ams.len() - 2]
    } else {
        &canon_ams
    };
    ams_signed_block.extend_from_slice(canon_ams_trimmed);

    let ams_sig = sign_signature(&key.as_crypto(), &ams_signed_block)
        .map_err(|e| ArcError::InvalidPublicKey(format!("AMS sign: {e}")))?;
    let ams_sig_b64 = base64::engine::general_purpose::STANDARD.encode(&ams_sig);
    let ams_line = format!("ARC-Message-Signature: {ams_tags}{ams_sig_b64}\r\n");

    // 4. Build AS — always relaxed/relaxed canon (RFC 8617 §5.1.1).
    let cv = cv_str(opts.cv);
    let mut as_tags = format!(
        "i={new_i}; a={alg}; cv={cv}; d={d}; s={s}",
        d = opts.domain,
        s = opts.selector,
    );
    if let Some(t) = opts.timestamp {
        as_tags = format!("{as_tags}; t={t}");
    }
    as_tags = format!("{as_tags}; b=");

    // AS signed input — concat in (AAR, AMS, AS) order for each prior
    // set + the new (AAR, AMS, AS with b= empty).
    let mut as_signed_block = Vec::with_capacity(1024);
    if let Some(chain) = prior {
        for set in &chain.sets {
            as_signed_block.extend_from_slice(&canonicalize_header(
                "ARC-Authentication-Results",
                &set.raw_aar,
                DkimCanon::Relaxed,
            ));
            as_signed_block.extend_from_slice(&canonicalize_header(
                "ARC-Message-Signature",
                &set.raw_ams,
                DkimCanon::Relaxed,
            ));
            as_signed_block.extend_from_slice(&canonicalize_header(
                "ARC-Seal",
                &set.raw_seal,
                DkimCanon::Relaxed,
            ));
        }
    }
    // New AAR + AMS contribute full values; the new AS has b= cleared.
    // raw_aar / raw_ams here are the values WITHOUT the "Name:" prefix
    // (matching what find_header_value would return for verify), so
    // pull them out the same way.
    let new_aar_value = aar_line
        .strip_prefix("ARC-Authentication-Results:")
        .map(|s| s.trim_end_matches("\r\n"))
        .unwrap_or(&aar_value);
    let new_ams_value = ams_line
        .strip_prefix("ARC-Message-Signature:")
        .map(|s| s.trim_end_matches("\r\n"))
        .unwrap_or(&ams_tags);
    as_signed_block.extend_from_slice(&canonicalize_header(
        "ARC-Authentication-Results",
        new_aar_value,
        DkimCanon::Relaxed,
    ));
    as_signed_block.extend_from_slice(&canonicalize_header(
        "ARC-Message-Signature",
        new_ams_value,
        DkimCanon::Relaxed,
    ));
    // Current AS with b= cleared (it's already empty; clear_b_value
    // is idempotent on that input — calling it for symmetry).
    let as_value = format!(" {as_tags}");
    let as_cleared = clear_b_value(&as_value);
    let canon_as = canonicalize_header("ARC-Seal", &as_cleared, DkimCanon::Relaxed);
    let canon_as_trimmed = if canon_as.ends_with(b"\r\n") {
        &canon_as[..canon_as.len() - 2]
    } else {
        &canon_as
    };
    as_signed_block.extend_from_slice(canon_as_trimmed);

    let as_sig = sign_signature(&key.as_crypto(), &as_signed_block)
        .map_err(|e| ArcError::InvalidPublicKey(format!("AS sign: {e}")))?;
    let as_sig_b64 = base64::engine::general_purpose::STANDARD.encode(&as_sig);
    let as_line = format!("ARC-Seal: {as_tags}{as_sig_b64}\r\n");

    Ok(SealedHeaders {
        aar: aar_line,
        ams: ams_line,
        seal: as_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_canon_roundtrips() {
        assert_eq!(map_canon(Canon::Simple), DkimCanon::Simple);
        assert_eq!(map_canon(Canon::Relaxed), DkimCanon::Relaxed);
    }

    #[test]
    fn alg_str_emits_spec_strings() {
        assert_eq!(alg_str(Algorithm::RsaSha256), "rsa-sha256");
        assert_eq!(alg_str(Algorithm::Ed25519Sha256), "ed25519-sha256");
    }

    #[test]
    fn cv_str_emits_spec_strings() {
        assert_eq!(cv_str(ArcSealCv::None), "none");
        assert_eq!(cv_str(ArcSealCv::Pass), "pass");
        assert_eq!(cv_str(ArcSealCv::Fail), "fail");
    }

    #[test]
    fn sealed_headers_concat_order_is_aar_ams_seal() {
        let s = SealedHeaders {
            aar: "AAR\r\n".into(),
            ams: "AMS\r\n".into(),
            seal: "AS\r\n".into(),
        };
        assert_eq!(s.concat(), "AAR\r\nAMS\r\nAS\r\n");
    }

    #[test]
    fn signing_key_algorithm_helper() {
        let secret = [0u8; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&secret);
        let key = ArcSigningKey::Ed25519(&sk);
        assert_eq!(key.algorithm(), Algorithm::Ed25519Sha256);
    }
}
