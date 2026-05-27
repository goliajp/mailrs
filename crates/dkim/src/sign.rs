//! DKIM signer (RFC 6376 §3.7).
//!
//! Produces a `DKIM-Signature:` header VALUE ready to prepend to an
//! outbound message. Caller owns the key (RSA or Ed25519); we apply
//! body + header canonicalization, hash, sign, and assemble the
//! header tag list.
//!
//! ## Why this lives next to verify
//!
//! Sign and verify run the same canonicalization (`mailrs_dkim::canon`)
//! and the same hash + signature primitive (`mailrs_dkim::crypto`).
//! Splitting them into separate crates would force outbound senders
//! to depend on both — and any drift between sign and verify would
//! silently corrupt every signature this crate produces. Keeping
//! them co-located + sharing one canon implementation is the only
//! design that stays correct under refactor.
//!
//! ## Algorithm choice is implied by the key
//!
//! [`DkimSigningKey::Rsa`] → `a=rsa-sha256`.
//! [`DkimSigningKey::Ed25519`] → `a=ed25519-sha256` (RFC 8463).
//! There is no `a=` tag in [`SignOpts`] — pass the right key.

use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::canon::{canonicalize_body, canonicalize_header};
use crate::crypto::{CryptoSigningKey, RsaSigningKey, sign_signature};
use crate::error::DkimError;
use crate::header::{Algorithm, Canon};
use crate::headers::{body_offset_minus_blank, find_body_offset};

/// Private key used to sign a DKIM-Signature. Algorithm is implied by
/// the variant.
#[allow(
    clippy::large_enum_variant,
    reason = "ed25519_dalek::SigningKey holds a cached scalar (~64-96 bytes); \
              RsaSigningKey is an Arc + usize (~16 bytes). Boxing the larger \
              variant would mean a heap alloc per DkimSigningKey::Ed25519 \
              construction. The enum is created once per outbound mail at \
              most and not stored in large collections, so the size penalty \
              is benign."
)]
pub enum DkimSigningKey {
    /// RSA private key — produces `a=rsa-sha256`. Mainstream choice;
    /// supports 1024 / 2048 / 4096-bit moduli. Construct via
    /// [`RsaSigningKey::from_pkcs8_pem`] (or `_der`).
    Rsa(RsaSigningKey),
    /// Ed25519 signing key — produces `a=ed25519-sha256` (RFC 8463).
    /// Smaller signatures, faster signing, but selector TXT records
    /// publish the raw 32-byte key (NOT PKCS8) so receivers need to
    /// handle both formats.
    Ed25519(ed25519_dalek::SigningKey),
}

impl DkimSigningKey {
    /// Return the `Algorithm` this key signs with.
    pub fn algorithm(&self) -> Algorithm {
        match self {
            Self::Rsa(_) => Algorithm::RsaSha256,
            Self::Ed25519(_) => Algorithm::Ed25519Sha256,
        }
    }
}

/// Options for [`sign`].
#[derive(Debug, Clone)]
pub struct SignOpts {
    /// `d=` — signing domain.
    pub domain: String,
    /// `s=` — selector (publishes the public key at
    /// `<selector>._domainkey.<domain>`).
    pub selector: String,
    /// `h=` — header NAMES to sign, in order. Case-insensitive
    /// lookup against the message. Headers that don't exist on the
    /// message are skipped (RFC 6376 §3.5).
    pub signed_headers: Vec<String>,
    /// `c=` header canonicalization.
    pub canon_header: Canon,
    /// `c=` body canonicalization.
    pub canon_body: Canon,
    /// `i=` — optional identity (AUID).
    pub identity: Option<String>,
    /// `t=` — optional signature timestamp. Caller supplies because
    /// this crate avoids the `std::time::SystemTime` dep on the
    /// signing path (lets tests pin time).
    pub timestamp: Option<u64>,
    /// `x=` — optional expiration timestamp.
    pub expiration: Option<u64>,
    /// `l=` — optional body-length limit. Most signers leave this
    /// unset; a few sign just the first N bytes to tolerate
    /// trailing additions.
    pub body_length: Option<u64>,
}

impl SignOpts {
    /// Sensible defaults: relaxed/relaxed canon, no expiry, no AUID,
    /// no body-length limit. Caller still must set `domain`,
    /// `selector`, and `signed_headers`.
    pub fn new(domain: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            selector: selector.into(),
            signed_headers: Vec::new(),
            canon_header: Canon::Relaxed,
            canon_body: Canon::Relaxed,
            identity: None,
            timestamp: None,
            expiration: None,
            body_length: None,
        }
    }

    /// Add a single header name to the `h=` list.
    pub fn add_signed_header(mut self, name: impl Into<String>) -> Self {
        self.signed_headers.push(name.into());
        self
    }

    /// Replace the `h=` list with the provided one.
    pub fn signed_headers<I: IntoIterator<Item = S>, S: Into<String>>(
        mut self,
        headers: I,
    ) -> Self {
        self.signed_headers = headers.into_iter().map(Into::into).collect();
        self
    }

    /// Set `t=`.
    pub fn timestamp(mut self, t: u64) -> Self {
        self.timestamp = Some(t);
        self
    }

    /// Set `x=`.
    pub fn expiration(mut self, x: u64) -> Self {
        self.expiration = Some(x);
        self
    }
}

/// Sign `raw_message` and return the full `DKIM-Signature:` header
/// LINE ready to prepend to the message (`"DKIM-Signature: ...\r\n"`).
///
/// Steps (RFC 6376 §3.7):
/// 1. Locate body offset.
/// 2. Canonicalize body, SHA-256 → `bh=`.
/// 3. Construct the DKIM-Signature header value with `b=` empty.
/// 4. Canonicalize each `h=` header (when present) + the DKIM-Signature
///    itself (`b=` empty), strip trailing CRLF on the DKIM-Signature,
///    that's the signed-header block.
/// 5. Hash + sign with the supplied key.
/// 6. Substitute `b=<sig>` into the header value.
///
/// Returns the full `DKIM-Signature: <tag list>\r\n` line. To attach
/// to a message, prepend it to the existing wire bytes.
///
/// # Errors
///
/// Returns [`DkimError::MissingHeader`] if `raw_message` has no
/// CRLF CRLF / LF LF body separator. Bubbles signing failures from
/// the `rsa` / `ed25519_dalek` crates as [`DkimError::SignatureMismatch`]
/// (sign failures are exceptionally rare — only happens on truly
/// malformed keys).
pub fn sign(
    raw_message: &[u8],
    key: &DkimSigningKey,
    opts: &SignOpts,
) -> Result<String, DkimError> {
    // 1. Body offset + canonicalize body + bh=.
    let body_offset = find_body_offset(raw_message).ok_or(DkimError::MissingHeader)?;
    let body = &raw_message[body_offset..];
    let canon_body_bytes = canonicalize_body(body, opts.canon_body, opts.body_length);
    let mut body_hasher = Sha256::new();
    body_hasher.update(&canon_body_bytes);
    let bh = base64::engine::general_purpose::STANDARD.encode(body_hasher.finalize());

    // 2. Build a tag list with b= LAST and empty. b= must remain the
    // final tag so we can append the signature bytes directly without
    // disturbing any other tag's value. Optional tags (i, t, x, l) go
    // BEFORE b=, in a canonical order so the wire format is stable.
    let alg_str = match key.algorithm() {
        Algorithm::RsaSha256 => "rsa-sha256",
        Algorithm::Ed25519Sha256 => "ed25519-sha256",
    };
    let canon_str = canon_pair(opts.canon_header, opts.canon_body);
    let h_list = opts.signed_headers.join(":");
    let mut tags = format!(
        "v=1; a={alg_str}; c={canon_str}; d={d}; s={s}; h={h}; bh={bh}",
        d = opts.domain,
        s = opts.selector,
        h = h_list,
    );
    if let Some(i) = &opts.identity {
        tags = format!("{tags}; i={i}");
    }
    if let Some(t) = opts.timestamp {
        tags = format!("{tags}; t={t}");
    }
    if let Some(x) = opts.expiration {
        tags = format!("{tags}; x={x}");
    }
    if let Some(l) = opts.body_length {
        tags = format!("{tags}; l={l}");
    }
    tags = format!("{tags}; b=");

    // 3. Compute the signed-header block.
    //
    // Header name handling: verify's `DkimHeader::parse` lowercases
    // every name in `h=` for normalization, so when the verifier
    // canonicalizes each signed header it uses the lowercased name.
    // Sign MUST mirror that — otherwise SIMPLE canon (which
    // preserves the name verbatim per RFC 6376 §3.4.1) produces
    // "From:" while verify produces "from:" and the hashes don't
    // match. For RELAXED canon `canonicalize_header` lowercases the
    // name itself so this is a no-op, but doing it unconditionally
    // keeps the contract self-consistent.
    let headers_region = &raw_message[..body_offset_minus_blank(body_offset, raw_message)];
    let mut signed_block = Vec::with_capacity(512);
    // Per RFC 6376 §5.4.2: walk h= in order, consume one occurrence
    // each scanning bottom-up. Repeated entries for which no fresh
    // occurrence exists are SKIPPED — not emitted as a null header.
    // Matches OpenDKIM + stalwart/mail-auth convention; an emitted
    // `from:\r\n` would corrupt the hash on the verify side.
    let collected = crate::headers::collect_signed_headers(headers_region, &opts.signed_headers);
    for (name, value_opt) in &collected {
        let Some(value) = value_opt else { continue };
        let canon_name = name.to_ascii_lowercase();
        signed_block.extend_from_slice(&canonicalize_header(
            &canon_name,
            value,
            opts.canon_header,
        ));
    }
    // Append the DKIM-Signature header value itself with b= empty.
    // canonicalize_header takes a name + value; the value here is the
    // tag list we just built (which ends in "b="), and the canonical
    // form includes the "DKIM-Signature:" prefix.
    //
    // IMPORTANT: prepend a leading space to match exactly what
    // verify_one sees. The wire format we emit is
    // `DKIM-Signature: <tags>\r\n` (space after colon), and verify's
    // `find_header_value_in_raw` returns the value with that leading
    // space preserved. For SIMPLE canon, simple-canon preserves the
    // value verbatim — so without this prepend, sign hashes
    // "DKIM-Signature:<tags>" and verify hashes "DKIM-Signature: <tags>".
    // Mismatch ⇒ every simple/simple signature unverifiable.
    //
    // We do NOT append a trailing `\r` here even though verify's
    // value comes with one: `clear_b_value` on the verify side
    // consumes everything between `b=` and the next `;` (or end of
    // string), which strips the wire-format `\r` along with the
    // signature bytes. So the cleared value on the verify side
    // has the same trailing-CR-free shape as our `tags`.
    let signed_value = format!(" {tags}");
    let canon_dkim = canonicalize_header("DKIM-Signature", &signed_value, opts.canon_header);
    // Per §3.7: trailing CRLF of the DKIM-Signature is NOT in the
    // hash input.
    let canon_dkim_trimmed = if canon_dkim.ends_with(b"\r\n") {
        &canon_dkim[..canon_dkim.len() - 2]
    } else {
        &canon_dkim
    };
    signed_block.extend_from_slice(canon_dkim_trimmed);

    // 4. Sign via the standalone crypto primitive — same one ARC
    // uses for seal signing, so any drift would break verify in
    // both crates simultaneously.
    let crypto_key = match key {
        DkimSigningKey::Rsa(k) => CryptoSigningKey::Rsa(k),
        DkimSigningKey::Ed25519(k) => CryptoSigningKey::Ed25519(k),
    };
    let sig = sign_signature(&crypto_key, &signed_block)?;
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(&sig);

    // 5. Substitute b= value and emit the full header line.
    Ok(format!("DKIM-Signature: {tags}{sig_b64}\r\n"))
}

fn canon_pair(h: Canon, b: Canon) -> &'static str {
    match (h, b) {
        (Canon::Simple, Canon::Simple) => "simple/simple",
        (Canon::Simple, Canon::Relaxed) => "simple/relaxed",
        (Canon::Relaxed, Canon::Simple) => "relaxed/simple",
        (Canon::Relaxed, Canon::Relaxed) => "relaxed/relaxed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canon_pair_emits_spec_strings() {
        assert_eq!(canon_pair(Canon::Simple, Canon::Simple), "simple/simple");
        assert_eq!(canon_pair(Canon::Simple, Canon::Relaxed), "simple/relaxed");
        assert_eq!(canon_pair(Canon::Relaxed, Canon::Simple), "relaxed/simple");
        assert_eq!(
            canon_pair(Canon::Relaxed, Canon::Relaxed),
            "relaxed/relaxed"
        );
    }

    #[test]
    fn signing_key_algorithm_matches_variant() {
        // Construct a tiny dummy ed25519 key (rand-free).
        let secret = [42u8; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&secret);
        let key = DkimSigningKey::Ed25519(sk);
        assert_eq!(key.algorithm(), Algorithm::Ed25519Sha256);
    }

    #[test]
    fn sign_opts_builder_chain() {
        let opts = SignOpts::new("example.com", "s1")
            .add_signed_header("From")
            .add_signed_header("Subject")
            .timestamp(1_700_000_000)
            .expiration(1_700_086_400);
        assert_eq!(opts.domain, "example.com");
        assert_eq!(opts.selector, "s1");
        assert_eq!(opts.signed_headers, vec!["From", "Subject"]);
        assert_eq!(opts.timestamp, Some(1_700_000_000));
        assert_eq!(opts.expiration, Some(1_700_086_400));
    }

    #[test]
    fn sign_errors_on_missing_body_separator() {
        let secret = [1u8; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&secret);
        let key = DkimSigningKey::Ed25519(sk);
        let opts = SignOpts::new("example.com", "s1").add_signed_header("From");
        // No "\r\n\r\n" — find_body_offset returns None.
        let msg = b"From: a@b.c\r\nSubject: hi\r\n";
        assert!(matches!(
            sign(msg, &key, &opts),
            Err(DkimError::MissingHeader)
        ));
    }

    // End-to-end roundtrip with a real RSA-2048 key lives in
    // `tests/sign_roundtrip.rs` because it needs a real RNG +
    // verify_all to confirm we generated a self-consistent signature.
}
