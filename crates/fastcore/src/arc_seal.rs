//! ARC sealing (RFC 8617) for mail we forward.
//!
//! When we forward a message — a sieve `redirect`, an alias that points
//! off-site — the original SPF breaks (the next hop sees our IP, not the
//! sender's) and DKIM breaks the moment anything rewrites a signed
//! header. ARC lets us attach what *we* saw at receipt time, so a
//! downstream receiver that trusts us can recover the original verdict
//! instead of failing the message.
//!
//! ## Only the first hop
//!
//! If the message already carries an ARC chain, this module leaves it
//! alone rather than adding a hop.
//!
//! Adding a hop means emitting `cv=pass` or `cv=fail`, and that is an
//! assertion about every prior hop's cryptography. Verifying it
//! properly costs one DNS lookup per hop, on the delivery path, for a
//! message that is already in flight. Two things follow: we will not
//! assert what we have not verified, and we will not put a chain of DNS
//! lookups between a message and its next hop. Sealing only where the
//! answer is knowable without lookups — the first hop, where `cv=none`
//! is correct by definition — keeps both.
//!
//! In practice we are the first forwarder for essentially all of our
//! forwarded mail, so this covers the real traffic.
//!
//! ## Never a delivery blocker
//!
//! Every failure path returns `None` and the message goes out unsealed.
//! An unsealed forward is a message that might fail DMARC downstream;
//! a blocked forward is a message that definitely never arrives.

use mailrs_arc::{ArcChain, ArcSealCv, ArcSigningKey, Canon, SealOpts, seal};

/// Headers covered by the ARC-Message-Signature.
///
/// Same set the DKIM signer uses. Keeping them identical means a
/// downstream verifier sees the two signatures agree on what was
/// protected.
const SIGNED_HEADERS: [&str; 5] = ["From", "To", "Subject", "Date", "Message-ID"];

/// Add an ARC set to a message we are forwarding.
///
/// Returns the message with `ARC-Authentication-Results`,
/// `ARC-Message-Signature` and `ARC-Seal` prepended, or `None` when
/// sealing does not apply — no authentication results to vouch for, an
/// existing chain (see module docs), or any signing error.
pub fn seal_forwarded(
    message: &[u8],
    rsa: &mailrs_dkim::RsaSigningKey,
    domain: &str,
    selector: &str,
) -> Option<Vec<u8>> {
    // Nothing to vouch for if the receiver never stamped a verdict.
    // Sealing an empty authres would assert "we checked and found
    // nothing", which is not what happened.
    let authres = raw_authentication_results(message)?;
    if authres.trim().is_empty() {
        return None;
    }

    match ArcChain::extract(message) {
        Ok(None) => {}
        Ok(Some(_)) => {
            tracing::debug!("ARC chain already present; not adding a hop");
            return None;
        }
        Err(e) => {
            tracing::debug!(error = %e, "ARC chain unparseable; forwarding unsealed");
            return None;
        }
    }

    let opts = SealOpts {
        domain: domain.to_string(),
        selector: selector.to_string(),
        signed_headers: SIGNED_HEADERS.iter().map(|s| s.to_string()).collect(),
        canon_header: Canon::Relaxed,
        canon_body: Canon::Relaxed,
        // First hop: cv=none is the only valid value (RFC 8617 §5.1).
        cv: ArcSealCv::None,
        authres,
        timestamp: None,
    };

    match seal(message, &ArcSigningKey::Rsa(rsa), &opts, None) {
        Ok(headers) => {
            let block = headers.concat();
            let mut out = Vec::with_capacity(block.len() + message.len());
            out.extend_from_slice(block.as_bytes());
            out.extend_from_slice(message);
            tracing::info!(arc_d = %domain, "sealed forwarded message");
            Some(out)
        }
        Err(e) => {
            tracing::warn!(error = %e, "ARC seal failed; forwarding unsealed");
            None
        }
    }
}

/// Read the raw `Authentication-Results` field value, unfolding
/// continuation lines.
///
/// Returns `None` when the header is absent — which happens for mail
/// that reached us by a path that does not stamp one.
pub(crate) fn raw_authentication_results(raw: &[u8]) -> Option<String> {
    let head = &raw[..raw.len().min(16 * 1024)];
    let text = String::from_utf8_lossy(head);
    let mut value: Option<String> = None;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        match value.as_mut() {
            Some(v) => {
                // A folded continuation starts with whitespace; anything
                // else ends the field.
                if line.starts_with(' ') || line.starts_with('\t') {
                    v.push(' ');
                    v.push_str(line.trim());
                    continue;
                }
                break;
            }
            None => {
                if let Some(rest) = line
                    .strip_prefix("Authentication-Results:")
                    .or_else(|| line.strip_prefix("authentication-results:"))
                {
                    value = Some(rest.trim().to_string());
                }
            }
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    const AR: &str = "Authentication-Results: mx.golia.ai; spf=pass smtp.mailfrom=a@b.c; \
                      dkim=pass header.d=b.c; dmarc=pass";

    fn msg_with(headers: &str) -> Vec<u8> {
        format!("{headers}From: a@b.c\r\nSubject: hi\r\n\r\nbody").into_bytes()
    }

    #[test]
    fn reads_the_authentication_results_value() {
        let m = msg_with(&format!("{AR}\r\n"));
        let v = raw_authentication_results(&m).expect("present");

        assert!(v.starts_with("mx.golia.ai;"));
        assert!(v.contains("dmarc=pass"));
    }

    #[test]
    fn unfolds_a_continued_header() {
        let m = msg_with("Authentication-Results: mx.golia.ai;\r\n\tspf=pass;\r\n\tdmarc=pass\r\n");
        let v = raw_authentication_results(&m).expect("present");

        assert_eq!(v, "mx.golia.ai; spf=pass; dmarc=pass");
        assert!(!v.contains('\n'), "must be a single line for the AAR");
    }

    #[test]
    fn absent_header_yields_none() {
        assert_eq!(raw_authentication_results(&msg_with("")), None);
    }

    #[test]
    fn stops_at_the_next_header() {
        let m = msg_with(&format!("{AR}\r\nX-Other: not part of authres\r\n"));
        let v = raw_authentication_results(&m).expect("present");

        assert!(!v.contains("X-Other"));
        assert!(!v.contains("not part of authres"));
    }

    #[test]
    fn a_message_without_authres_is_not_sealed() {
        // No key needed — the authres check short-circuits first.
        let m = msg_with("");
        assert!(raw_authentication_results(&m).is_none());
    }
}
