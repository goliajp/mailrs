//! End-to-end DKIM verifier: parse DKIM-Signature → fetch public key
//! → canonicalize → hash → verify signature.

use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::canon::{canonicalize_body, canonicalize_header};
use crate::crypto::{extract_public_key, verify_signature};
use crate::error::{DkimError, DkimResult};
use crate::header::DkimHeader;
use crate::headers::{
    body_offset_minus_blank, clear_b_value, find_body_offset, find_header_value,
    find_header_value_in_raw,
};
use crate::resolver::DkimResolver;

/// Verify a DKIM-signed message.
///
/// `raw_message` is the full RFC 5322 wire form (headers + CRLF +
/// body), exactly as it came off the wire. The verifier extracts the
/// first `DKIM-Signature:` header, parses it, looks up the public
/// key at `<selector>._domainkey.<domain>`, and validates the
/// signature.
///
/// Returns the seven RFC 8601 vocabulary values. Never panics. Errors
/// internal to verification (DNS failures, key parse errors) are
/// mapped to the appropriate `temperror`/`permerror`/`neutral` value.
pub async fn verify<R: DkimResolver + ?Sized>(resolver: &R, raw_message: &[u8]) -> DkimResult {
    match verify_inner(resolver, raw_message).await {
        Ok(r) => r,
        Err(e) => e.to_result(),
    }
}

async fn verify_inner<R: DkimResolver + ?Sized>(
    resolver: &R,
    raw_message: &[u8],
) -> Result<DkimResult, DkimError> {
    // 1. Locate the DKIM-Signature header value + body offset.
    let (header_value, signed_headers_raw, body_offset) = extract_dkim_signature(raw_message)?;
    let header = DkimHeader::parse(&header_value)?;

    // 2. Check expiry.
    if let Some(x) = header.expiration {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now > x {
            return Err(DkimError::Expired);
        }
    }

    // 3. Compute body hash on the canonicalized body and compare to bh=.
    let body = &raw_message[body_offset..];
    let canon_body_bytes = canonicalize_body(body, header.canon_body, header.body_length);
    let mut body_hasher = Sha256::new();
    body_hasher.update(&canon_body_bytes);
    let actual_body_hash = body_hasher.finalize();
    let expected_body_hash = base64::engine::general_purpose::STANDARD
        .decode(&header.body_hash_b64)
        .map_err(|_| DkimError::InvalidBase64("bh".into()))?;
    if actual_body_hash.as_slice() != expected_body_hash.as_slice() {
        return Err(DkimError::BodyHashMismatch);
    }

    // 4. Fetch + parse public key.
    let pubkey_domain = format!("{}._domainkey.{}", header.selector, header.domain);
    let txts = resolver.lookup_txt(&pubkey_domain).await?;
    if txts.is_empty() {
        return Err(DkimError::DnsPermError(format!(
            "no TXT at {pubkey_domain}"
        )));
    }
    // Pick the first record containing `p=` (most have only one).
    let key_txt = txts
        .iter()
        .find(|s| s.contains("p="))
        .ok_or_else(|| DkimError::InvalidKey("no p= tag in TXT".into()))?;
    let key_bytes = extract_public_key(key_txt)?;

    // 5. Compute the canonicalized signed-header block (per RFC 6376
    //    §3.7, the signed headers are emitted in the order listed by
    //    h=, then the DKIM-Signature header itself with b= empty).
    let mut signed_block = Vec::new();
    for name in &header.signed_headers {
        if let Some(value) = find_header_value(signed_headers_raw, name) {
            let canon = canonicalize_header(name, value, header.canon_header);
            signed_block.extend_from_slice(&canon);
        }
        // Missing signed header → skip (per §3.5: only signed headers
        // that actually exist contribute). This means a malicious
        // signer can list non-existent headers; the absence is its own
        // signal but doesn't break verification.
    }
    // Append the DKIM-Signature header itself with `b=` value cleared.
    let dkim_sig_b_cleared = clear_b_value(&header_value);
    let canon_dkim = canonicalize_header(
        "DKIM-Signature",
        &dkim_sig_b_cleared,
        header.canon_header,
    );
    // Per RFC 6376 §3.7: the trailing CRLF of the DKIM-Signature is
    // NOT included in the hash input. Strip it.
    let canon_dkim_trimmed = if canon_dkim.ends_with(b"\r\n") {
        &canon_dkim[..canon_dkim.len() - 2]
    } else {
        &canon_dkim
    };
    signed_block.extend_from_slice(canon_dkim_trimmed);

    // 6. Verify RSA-SHA256 / Ed25519-SHA256 signature.
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(&header.signature_b64)
        .map_err(|_| DkimError::InvalidBase64("b".into()))?;
    verify_signature(
        header.algorithm,
        &key_bytes,
        &signed_block,
        &signature_bytes,
    )?;

    Ok(DkimResult::Pass)
}

/// Find the DKIM-Signature header value + return (value, raw-headers-region, body-offset).
fn extract_dkim_signature(raw: &[u8]) -> Result<(String, &[u8], usize), DkimError> {
    // Locate the empty-line terminator separating headers from body.
    let body_offset = find_body_offset(raw).ok_or(DkimError::MissingHeader)?;
    let headers_raw = &raw[..body_offset_minus_blank(body_offset, raw)];
    // Find the DKIM-Signature: header line and its full (possibly folded) value.
    let value = find_header_value_in_raw(headers_raw, b"DKIM-Signature")?;
    Ok((value, headers_raw, body_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_dkim_signature_finds_header_and_body_offset() {
        let raw = b"DKIM-Signature: v=1; a=rsa-sha256\r\nFrom: a\r\n\r\nbody";
        let (value, headers_raw, body_offset) = extract_dkim_signature(raw).unwrap();
        assert_eq!(value, " v=1; a=rsa-sha256\r");
        assert!(headers_raw.starts_with(b"DKIM-Signature"));
        assert_eq!(&raw[body_offset..], b"body");
    }

    #[test]
    fn extract_dkim_signature_errors_when_missing() {
        let raw = b"From: a\r\n\r\nbody";
        assert!(extract_dkim_signature(raw).is_err());
    }
}
