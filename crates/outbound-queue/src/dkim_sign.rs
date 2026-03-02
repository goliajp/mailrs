use mail_auth::arc::ArcSealer;
use mail_auth::common::crypto::{RsaKey, Sha256};
use mail_auth::common::headers::HeaderWriter;
use mail_auth::dkim::DkimSigner;
use mail_auth::{AuthenticatedMessage, AuthenticationResults, MessageAuthenticator};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

/// DKIM signing configuration
#[derive(Debug, Clone)]
pub struct DkimSignConfig {
    pub selector: String,
    pub domain: String,
    pub private_key_pem: String,
}

impl DkimSignConfig {
    /// sign a message, prepending the DKIM-Signature header
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        let pkcs8 = PrivatePkcs8KeyDer::from_pem_slice(self.private_key_pem.as_bytes())
            .map_err(|e| format!("failed to parse DKIM PEM: {e}"))?;
        let key = RsaKey::<Sha256>::from_key_der(PrivateKeyDer::Pkcs8(pkcs8))
            .map_err(|e| format!("failed to load DKIM key: {e}"))?;

        let signature = DkimSigner::from_key(key)
            .domain(&self.domain)
            .selector(&self.selector)
            .headers(["From", "To", "Subject", "Date", "Message-ID"])
            .sign(message)
            .map_err(|e| format!("DKIM signing failed: {e}"))?;

        let header = signature.to_header();
        let mut signed = Vec::with_capacity(header.len() + message.len());
        signed.extend_from_slice(header.as_bytes());
        signed.extend_from_slice(message);
        Ok(signed)
    }
}

/// extract domain from email address
pub fn extract_domain(email: &str) -> Option<&str> {
    email.rsplit_once('@').map(|(_, domain)| domain)
}

/// ARC-seal a forwarded message, preserving authentication chain (RFC 8617)
pub async fn arc_seal_message(
    dkim_config: &DkimSignConfig,
    authenticator: &MessageAuthenticator,
    message: &[u8],
) -> Result<Vec<u8>, String> {
    let auth_msg = AuthenticatedMessage::parse(message)
        .ok_or("failed to parse message for ARC sealing")?;

    // verify existing DKIM signatures (for auth results)
    let dkim_results = authenticator.verify_dkim(&auth_msg).await;

    // verify existing ARC chain
    let arc_output = authenticator.verify_arc(&auth_msg).await;
    if !arc_output.can_be_sealed() {
        return Err("ARC chain cannot be sealed (invalid chain)".into());
    }

    // build Authentication-Results for ARC-Authentication-Results header
    let header_from = auth_msg.from();
    let auth_results = AuthenticationResults::new(&dkim_config.domain)
        .with_dkim_results(&dkim_results, header_from);

    // create ARC seal using the DKIM key
    let pkcs8 = PrivatePkcs8KeyDer::from_pem_slice(dkim_config.private_key_pem.as_bytes())
        .map_err(|e| format!("failed to parse DKIM PEM for ARC: {e}"))?;
    let key = RsaKey::<Sha256>::from_key_der(PrivateKeyDer::Pkcs8(pkcs8))
        .map_err(|e| format!("failed to load key for ARC: {e}"))?;

    let arc_set = ArcSealer::from_key(key)
        .domain(&dkim_config.domain)
        .selector(&dkim_config.selector)
        .headers(["From", "To", "Subject", "Date", "Message-ID", "DKIM-Signature"])
        .seal(&auth_msg, &auth_results, &arc_output)
        .map_err(|e| format!("ARC sealing failed: {e}"))?;

    // prepend ARC headers to message
    let arc_header = arc_set.to_header();
    let ar_header = auth_results.to_header();
    let mut sealed = Vec::with_capacity(arc_header.len() + ar_header.len() + message.len());
    sealed.extend_from_slice(arc_header.as_bytes());
    sealed.extend_from_slice(ar_header.as_bytes());
    sealed.extend_from_slice(message);
    Ok(sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_domain_valid() {
        assert_eq!(extract_domain("user@example.com"), Some("example.com"));
    }

    #[test]
    fn extract_domain_no_at() {
        assert_eq!(extract_domain("nope"), None);
    }
}
