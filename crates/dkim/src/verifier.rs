//! End-to-end DKIM verifier: parse DKIM-Signature → fetch public key
//! → canonicalize → hash → verify signature.

use base64::Engine as _;
use rsa::pkcs8::DecodePublicKey;
use rsa::Pkcs1v15Sign;
use sha2::{Digest, Sha256};

use crate::canon::{canonicalize_body, canonicalize_header};
use crate::error::{DkimError, DkimResult};
use crate::header::{Algorithm, DkimHeader};
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
    let key_pem_der = extract_public_key(key_txt)?;
    let public_key =
        rsa::RsaPublicKey::from_public_key_der(&key_pem_der).map_err(|e| {
            DkimError::InvalidKey(format!("RSA PKCS8 decode failed: {e}"))
        })?;

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

    // 6. Verify RSA-SHA256 signature.
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(&header.signature_b64)
        .map_err(|_| DkimError::InvalidBase64("b".into()))?;

    match header.algorithm {
        Algorithm::RsaSha256 => {
            // Hash the signed-header block ourselves, then call the
            // low-level Pkcs1v15Sign::verify. This avoids the
            // `VerifyingKey<Sha256>` generic that's sensitive to
            // sha2 version skew between workspace and rsa's traits.
            let mut hasher = Sha256::new();
            hasher.update(&signed_block);
            let digest = hasher.finalize();
            let scheme = Pkcs1v15Sign::new::<Sha256>();
            public_key
                .verify(scheme, &digest, &signature_bytes)
                .map_err(|_| DkimError::SignatureMismatch)?;
        }
    }

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

fn body_offset_minus_blank(body_offset: usize, raw: &[u8]) -> usize {
    // The blank-line CRLF (and possibly preceding CRLF on the last
    // header) sits BEFORE body_offset. We want the headers-region
    // ending at the last header's terminating CRLF, exclusive of the
    // blank line. body_offset points to first body byte. The blank
    // line is "\r\n" or "\n" before it.
    if body_offset >= 2 && &raw[body_offset - 2..body_offset] == b"\r\n" {
        body_offset - 2
    } else if body_offset >= 1 && raw[body_offset - 1] == b'\n' {
        body_offset - 1
    } else {
        body_offset
    }
}

fn find_body_offset(raw: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < raw.len() {
        // CRLF CRLF
        if i + 3 < raw.len() && &raw[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
        // LF LF (tolerate lone-LF systems)
        if i + 1 < raw.len() && raw[i] == b'\n' && raw[i + 1] == b'\n' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

/// Find a header by name in the raw headers region, return the full
/// folded value (everything after the first `:` up to but not
/// including the line's CRLF + continuation).
fn find_header_value_in_raw(headers: &[u8], name: &[u8]) -> Result<String, DkimError> {
    let mut i = 0;
    while i < headers.len() {
        // Match `name:` at line start (case-insensitive)
        if i + name.len() < headers.len()
            && headers[i..i + name.len()].eq_ignore_ascii_case(name)
            && headers[i + name.len()] == b':'
        {
            // Found header. Extract its value (everything up to a
            // CRLF NOT followed by WSP).
            let value_start = i + name.len() + 1;
            let mut j = value_start;
            while j < headers.len() {
                // Stop at CRLF or LF not followed by WSP
                if headers[j] == b'\n' {
                    let after = j + 1;
                    if after < headers.len() && matches!(headers[after], b' ' | b'\t') {
                        // Folded continuation; keep going
                        j += 1;
                        continue;
                    }
                    // End of header value
                    return Ok(String::from_utf8_lossy(&headers[value_start..j]).into_owned());
                }
                j += 1;
            }
            // Reached end of headers without a terminating LF
            return Ok(String::from_utf8_lossy(&headers[value_start..j]).into_owned());
        }
        // Skip to next line
        while i < headers.len() && headers[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    Err(DkimError::MissingHeader)
}

/// Find a header value (folded) by name in the raw headers region.
/// Returns Some(value) on success or None if not found.
fn find_header_value<'a>(headers: &'a [u8], name: &str) -> Option<&'a str> {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < headers.len() {
        if i + bytes.len() < headers.len()
            && headers[i..i + bytes.len()].eq_ignore_ascii_case(bytes)
            && headers[i + bytes.len()] == b':'
        {
            let value_start = i + bytes.len() + 1;
            let mut j = value_start;
            while j < headers.len() {
                if headers[j] == b'\n' {
                    let after = j + 1;
                    if after < headers.len() && matches!(headers[after], b' ' | b'\t') {
                        j += 1;
                        continue;
                    }
                    // Strip trailing \r
                    let end = if j > value_start && headers[j - 1] == b'\r' {
                        j - 1
                    } else {
                        j
                    };
                    return std::str::from_utf8(&headers[value_start..end]).ok();
                }
                j += 1;
            }
            return std::str::from_utf8(&headers[value_start..j]).ok();
        }
        while i < headers.len() && headers[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    None
}

/// Remove the value of the `b=` tag, leaving the `b=` itself in place.
/// Used when computing the header hash — the spec says the signature
/// bytes themselves are not part of the input (chicken-and-egg).
fn clear_b_value(value: &str) -> String {
    // Find " b=" (or "; b=") and replace everything up to the next ";"
    // or end-of-string with empty.
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Match `b=` boundary: preceded by '; ' or ';' or start with optional WSP
        let is_b_start = if i + 1 < bytes.len() && bytes[i] == b'b' && bytes[i + 1] == b'=' {
            // Check previous non-WSP char is ';' or this is the start
            let mut k = i;
            while k > 0 {
                k -= 1;
                if !matches!(bytes[k], b' ' | b'\t' | b'\r' | b'\n') {
                    break;
                }
            }
            k == 0 || bytes[k] == b';'
        } else {
            false
        };
        if is_b_start {
            // Emit "b=" and skip until ';' or end
            out.extend_from_slice(b"b=");
            i += 2;
            while i < bytes.len() && bytes[i] != b';' {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse the public-key TXT record into PKCS8 DER bytes.
/// Format: `v=DKIM1; k=rsa; p=<base64-DER>` (RFC 6376 §3.6.1).
fn extract_public_key(txt: &str) -> Result<Vec<u8>, DkimError> {
    let p_value = txt
        .split(';')
        .find_map(|t| {
            let t = t.trim();
            t.strip_prefix("p=")
        })
        .ok_or_else(|| DkimError::InvalidKey("p= tag missing".into()))?;
    let p_value = p_value
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
        .collect::<String>();
    if p_value.is_empty() {
        return Err(DkimError::InvalidKey("p= empty (key revoked)".into()));
    }
    base64::engine::general_purpose::STANDARD
        .decode(p_value.as_bytes())
        .map_err(|e| DkimError::InvalidKey(format!("p= base64 decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_offset_simple_crlf_crlf() {
        let raw = b"From: a\r\nTo: b\r\n\r\nhello";
        let off = find_body_offset(raw).unwrap();
        assert_eq!(&raw[off..], b"hello");
    }

    #[test]
    fn body_offset_lf_lf() {
        let raw = b"From: a\nTo: b\n\nhello";
        let off = find_body_offset(raw).unwrap();
        assert_eq!(&raw[off..], b"hello");
    }

    #[test]
    fn body_offset_no_blank_line() {
        let raw = b"From: a\r\nTo: b\r\n";
        assert!(find_body_offset(raw).is_none());
    }

    #[test]
    fn find_header_extracts_value() {
        let raw = b"From: alice@e.com\r\nTo: bob@e.com\r\n";
        assert_eq!(find_header_value(raw, "From"), Some(" alice@e.com"));
        assert_eq!(find_header_value(raw, "to"), Some(" bob@e.com"));
    }

    #[test]
    fn find_header_handles_folded() {
        let raw = b"X-Long: line1\r\n line2\r\nFrom: a\r\n";
        let val = find_header_value(raw, "X-Long").unwrap();
        assert!(val.contains("line1"));
        assert!(val.contains("line2"));
    }

    #[test]
    fn find_header_returns_none_if_absent() {
        let raw = b"From: a\r\n";
        assert!(find_header_value(raw, "Missing").is_none());
    }

    #[test]
    fn clear_b_replaces_value_only() {
        let v = " v=1; a=rsa-sha256; b=ABCDEFG; d=e.com";
        let cleared = clear_b_value(v);
        assert!(cleared.contains("b=;") || cleared.contains("b= "));
        assert!(!cleared.contains("ABCDEFG"));
        assert!(cleared.contains("v=1"));
        assert!(cleared.contains("a=rsa-sha256"));
        assert!(cleared.contains("d=e.com"));
    }

    #[test]
    fn extract_pubkey_finds_p_tag() {
        // Build a minimal valid base64 (just AA which decodes to one byte)
        let txt = "v=DKIM1; k=rsa; p=AA==";
        let der = extract_public_key(txt).unwrap();
        assert!(!der.is_empty());
    }

    #[test]
    fn extract_pubkey_rejects_missing_p() {
        let txt = "v=DKIM1; k=rsa";
        let r = extract_public_key(txt);
        assert!(matches!(r, Err(DkimError::InvalidKey(_))));
    }

    #[test]
    fn extract_pubkey_rejects_empty_p() {
        // Empty p= means the key was revoked
        let txt = "v=DKIM1; k=rsa; p=";
        let r = extract_public_key(txt);
        assert!(matches!(r, Err(DkimError::InvalidKey(_))));
    }

    #[test]
    fn extract_pubkey_strips_wsp_in_p() {
        let txt = "v=DKIM1; k=rsa; p=AA == ";
        let der = extract_public_key(txt).unwrap();
        assert!(!der.is_empty());
    }
}
