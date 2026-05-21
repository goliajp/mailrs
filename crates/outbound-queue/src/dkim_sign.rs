use mail_auth::arc::ArcSealer;
use mail_auth::common::crypto::{RsaKey, Sha256};
use mail_auth::common::headers::HeaderWriter;
use mail_auth::dkim::DkimSigner;
use mail_auth::{AuthenticatedMessage, AuthenticationResults, MessageAuthenticator};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

/// DKIM signing configuration.
#[derive(Debug, Clone)]
pub struct DkimSignConfig {
    /// DKIM selector — the label under `<selector>._domainkey.<domain>`.
    pub selector: String,
    /// Signing domain (matches the `d=` tag in the DKIM-Signature header).
    pub domain: String,
    /// Private RSA key in PKCS#8 PEM form.
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

    const TEST_RSA_KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDNZMkvBc/kAdQl\n\
GFY6ADYW+guQCJU4x6Zulb4/4fMDUHruL/DR722wV+qKmivIP5SS5X7H+U5X6xha\n\
1r70zJpdpEzyVZtctBZzm1BkKq81BVdL3iJCbVmPPqs2pUOjGsInmM7gEfvhz7CB\n\
q+RQ1fb9iGlBA/WmNqLKiVg1nEVDai6DHzEofI+Ta8ij5yGnYHVLJLsqmJotyvHN\n\
2vi/7kIFigjW/4TOQLcaZGm8AYTEBH4opqvb8C460vLjUFBHeoSqm0vkHWzrwNQx\n\
S29LczFc/WIpQkl1rx5iS8E5QI2u4eCHVElAjZp4IJsyPYBGVN72mi37IGfEjkHS\n\
O2TIUEQhAgMBAAECggEABK/ZlWydB1dxV11cTluF4HVZQTKo8RBBQIHDQyLtUDSM\n\
cZX/eVLs3lrLO9lzyVCGG+oHwBl0y7XOKvh+iAiJNzzSEq+YaX+kiYPQTFDbCasz\n\
CESr5HcpVYb5EjioN/ca2ht3EQ7oAAmkvfjFr4CKb9Omjzi/aMkTYurKbALCY9zk\n\
bx8J9VADe1aAAA54WFxIlJvb72Hrfw8iflFqVZNzykRp6tUvJJgSqLOpfM0ut5zb\n\
0ClgCjSZ7HpehjWVm3KBAOcC7p2TL3erpWoG9BuatgYLLRhW/AzLzXZ3/hSu9kEn\n\
ihws+VXkHxeaIafrck0HQyWnHb9QEcSgfVIhAYztlwKBgQD3O684316go2e6Qf4I\n\
7rF4JwmQiI+NMAQq55AwquZkfuw0N2F9AgyuzGskYvI9Ok+l/wP1e8Mb6JRuP6Nj\n\
dPYTQwzfmyZgdOovxGkZOGE60EQuX/1IS/NbLKQySAphgBVR2FlHnu+VMvha61tm\n\
/5K1ROAB3Ng3FbR7rHJXFjWU+wKBgQDUrUtS3Yj0yHnxA/AL04lsxNrLlinEVDM1\n\
6wPjC2VEXhj2j4JNrVqXG4GVYYEGhkUTjwcTOiZfmHaqMzEFo1aTOoiLrMMLQjmm\n\
jPNkLHsDXcbG5FA0BbzQmlj+ixKPToh2gHfeMfH96YmdROfmvY/TN9yI1FgkLErL\n\
YKatCKWokwKBgC6z25nGuD1oIMQSi0ZssKGd3jSrV1K4a1EfhSFsZzE8uKn0fDn9\n\
FSBABU1OU6w1Q657yeephWXUPZXF97tl8MYauGfVCx7Vdxem5qOY/uT5SqfoAhSS\n\
JFpoyGunKC7a3ywizlq1L1Tj1/50z0NZrAEKDbbMXRuqwflKzh6dV2nZAoGBAImh\n\
N6yBdr7J+bfRz4cntrgv0FONcqv9vUI4O0SzvC35Ivh0OGPiOkytXTd5aND7FTqq\n\
BW8Y43pbpPdRt3ipkj4m0/RnsbTYf4xbjKqX6mdsSVWurIRt7hmkuNDI2RLqRH9D\n\
dc7RzYN+nTKsQ9Jbe/a5ILtfh0apbyGcA2DYxrOHAoGAYYm/jwilVVaH1xSlP52w\n\
BcpT8g8Wqgo4wFOTcyGJScBeFnQO1dhap+KNxCOyM/b2a8p2kQxHPmhIt+iyUpsM\n\
Wob7+tvQ4QgOJAUWByTxMHczAY8Vrl45gxYS29ahbuvjtjPVLgHcaFnZPfun8i6u\n\
/qw9cba4IgRYuEuLJ9bzbAY=\n\
-----END PRIVATE KEY-----";

    fn test_config() -> DkimSignConfig {
        DkimSignConfig {
            selector: "test".into(),
            domain: "example.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
        }
    }

    fn simple_message() -> Vec<u8> {
        b"From: sender@example.com\r\n\
          To: recipient@example.com\r\n\
          Subject: Test\r\n\
          Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
          Message-ID: <test@example.com>\r\n\
          \r\n\
          Hello, world!\r\n"
            .to_vec()
    }

    // --- extract_domain tests ---

    #[test]
    fn extract_domain_valid() {
        assert_eq!(extract_domain("user@example.com"), Some("example.com"));
    }

    #[test]
    fn extract_domain_no_at() {
        assert_eq!(extract_domain("nope"), None);
    }

    #[test]
    fn extract_domain_empty_string() {
        assert_eq!(extract_domain(""), None);
    }

    #[test]
    fn extract_domain_at_only() {
        // "@" splits into ("", "") → domain is ""
        assert_eq!(extract_domain("@"), Some(""));
    }

    #[test]
    fn extract_domain_multiple_at_signs_uses_last() {
        // rsplit_once('@') takes the last '@'
        assert_eq!(extract_domain("user@host@example.com"), Some("example.com"));
    }

    #[test]
    fn extract_domain_subdomain() {
        assert_eq!(
            extract_domain("postmaster@mail.sub.example.com"),
            Some("mail.sub.example.com")
        );
    }

    #[test]
    fn extract_domain_local_only() {
        assert_eq!(extract_domain("no-domain@"), Some(""));
    }

    // --- error path tests ---

    #[test]
    fn dkim_sign_invalid_pem_returns_error() {
        let cfg = DkimSignConfig {
            selector: "selector".into(),
            domain: "example.com".into(),
            private_key_pem: "not-a-valid-pem".into(),
        };
        let result = cfg.sign(b"From: test@example.com\r\n\r\nbody");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("failed to parse DKIM PEM"), "unexpected error: {err}");
    }

    #[test]
    fn dkim_sign_empty_pem_returns_error() {
        let cfg = DkimSignConfig {
            selector: "sel".into(),
            domain: "example.com".into(),
            private_key_pem: String::new(),
        };
        let result = cfg.sign(b"From: test@example.com\r\n\r\n");
        assert!(result.is_err());
    }

    #[test]
    fn dkim_config_fields_stored() {
        let cfg = DkimSignConfig {
            selector: "myselector".into(),
            domain: "mydomain.com".into(),
            private_key_pem: "pem-data".into(),
        };
        assert_eq!(cfg.selector, "myselector");
        assert_eq!(cfg.domain, "mydomain.com");
        assert_eq!(cfg.private_key_pem, "pem-data");
    }

    // --- signature header format tests ---

    #[test]
    fn dkim_sign_prepends_dkim_signature_header() {
        let cfg = test_config();
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        let signed_str = String::from_utf8_lossy(&signed);
        assert!(
            signed_str.starts_with("DKIM-Signature:"),
            "signed message must start with DKIM-Signature header"
        );
    }

    #[test]
    fn dkim_signature_contains_required_tags() {
        let cfg = test_config();
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        let signed_str = String::from_utf8_lossy(&signed);

        // extract just the DKIM-Signature header (up to the original message)
        let dkim_header = signed_str
            .split_once("From: sender@example.com")
            .expect("original message must follow signature")
            .0;

        // RFC 6376 required tags
        assert!(dkim_header.contains("v=1"), "missing v= tag");
        assert!(dkim_header.contains("a=rsa-sha256"), "missing a= tag");
        assert!(dkim_header.contains("d=example.com"), "missing d= tag");
        assert!(dkim_header.contains("s=test"), "missing s= tag");
        assert!(dkim_header.contains("b="), "missing b= (signature) tag");
        assert!(dkim_header.contains("bh="), "missing bh= (body hash) tag");
        assert!(dkim_header.contains("h="), "missing h= (signed headers) tag");
    }

    #[test]
    fn dkim_signature_header_list_contains_signed_fields() {
        let cfg = test_config();
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        let signed_str = String::from_utf8_lossy(&signed);

        // find the h= tag value
        let h_start = signed_str.find("h=").expect("h= tag missing");
        let after_h = &signed_str[h_start..];
        // h= value ends at the next ';' or end of header
        let h_value = after_h
            .split_once(';')
            .map(|(v, _)| v)
            .unwrap_or(after_h);

        let h_lower = h_value.to_lowercase();
        for expected in ["from", "to", "subject", "date", "message-id"] {
            assert!(
                h_lower.contains(expected),
                "h= tag missing expected header: {expected}"
            );
        }
    }

    // --- original message preservation ---

    #[test]
    fn dkim_sign_preserves_original_message() {
        let cfg = test_config();
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        assert!(
            signed.ends_with(&msg),
            "signed output must end with the original message bytes"
        );
    }

    #[test]
    fn dkim_sign_output_is_larger_than_input() {
        let cfg = test_config();
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        assert!(
            signed.len() > msg.len(),
            "signed message must be larger due to prepended header"
        );
    }

    // --- determinism ---

    #[test]
    fn dkim_sign_same_input_produces_same_body_hash() {
        let cfg = test_config();
        let msg = simple_message();
        let signed1 = String::from_utf8_lossy(&cfg.sign(&msg).unwrap()).to_string();
        let signed2 = String::from_utf8_lossy(&cfg.sign(&msg).unwrap()).to_string();

        // extract bh= values
        let extract_bh = |s: &str| -> String {
            let start = s.find("bh=").unwrap() + 3;
            let end = s[start..].find(';').map(|i| start + i).unwrap_or(s.len());
            s[start..end].trim().to_string()
        };

        assert_eq!(
            extract_bh(&signed1),
            extract_bh(&signed2),
            "body hash must be deterministic for identical input"
        );
    }

    // --- empty body ---

    #[test]
    fn dkim_sign_empty_body() {
        let cfg = test_config();
        let msg = b"From: sender@example.com\r\n\
                     To: recipient@example.com\r\n\
                     Subject: Empty body\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <empty@example.com>\r\n\
                     \r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing an empty body must succeed");
        let signed = result.unwrap();
        let signed_str = String::from_utf8_lossy(&signed);
        assert!(signed_str.starts_with("DKIM-Signature:"));
    }

    #[test]
    fn dkim_sign_empty_body_has_known_body_hash() {
        // per RFC 6376, the body hash of an empty body (after canonicalization
        // of "\r\n") with sha-256 is always the same
        let cfg = test_config();
        let msg = b"From: a@example.com\r\n\r\n";
        let signed1 = cfg.sign(msg).unwrap();

        let msg2 = b"From: b@example.com\r\n\r\n";
        let signed2 = cfg.sign(msg2).unwrap();

        let extract_bh = |data: &[u8]| -> String {
            let s = String::from_utf8_lossy(data);
            let start = s.find("bh=").unwrap() + 3;
            let end = s[start..].find(';').map(|i| start + i).unwrap_or(s.len());
            s[start..end].trim().to_string()
        };

        assert_eq!(
            extract_bh(&signed1),
            extract_bh(&signed2),
            "empty body hash must be identical regardless of headers"
        );
    }

    // --- large message ---

    #[test]
    fn dkim_sign_large_message() {
        let cfg = test_config();
        let headers = b"From: sender@example.com\r\n\
                         To: recipient@example.com\r\n\
                         Subject: Large message test\r\n\
                         Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                         Message-ID: <large@example.com>\r\n\
                         \r\n";
        // ~1MB body
        let body_line = b"The quick brown fox jumps over the lazy dog. \r\n";
        let repeat_count = 1_000_000 / body_line.len();
        let mut msg = headers.to_vec();
        for _ in 0..repeat_count {
            msg.extend_from_slice(body_line);
        }

        let result = cfg.sign(&msg);
        assert!(result.is_ok(), "signing a ~1MB message must succeed");

        let signed = result.unwrap();
        assert!(signed.len() > msg.len());
        assert!(signed.ends_with(&msg));
    }

    // --- special characters in headers ---

    #[test]
    fn dkim_sign_utf8_subject() {
        let cfg = test_config();
        let msg = b"From: sender@example.com\r\n\
                     To: recipient@example.com\r\n\
                     Subject: =?UTF-8?B?5rWL6K+V5Li76aKY?=\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <utf8@example.com>\r\n\
                     \r\n\
                     body\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing with MIME-encoded UTF-8 subject must succeed");
    }

    #[test]
    fn dkim_sign_special_chars_in_subject() {
        let cfg = test_config();
        let msg = b"From: sender@example.com\r\n\
                     To: recipient@example.com\r\n\
                     Subject: Re: [PATCH v2] Fix \"bug\" in <module> & cleanup\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <special@example.com>\r\n\
                     \r\n\
                     body\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing with special chars in subject must succeed");
    }

    #[test]
    fn dkim_sign_long_folded_headers() {
        let cfg = test_config();
        // header folding per RFC 5322 — long header continued on next line
        let msg = b"From: sender@example.com\r\n\
                     To: recipient@example.com\r\n\
                     Subject: This is a very long subject line that should be folded\r\n\
                      across multiple lines to test header folding behavior\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <folded@example.com>\r\n\
                     \r\n\
                     body\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing with folded headers must succeed");
    }

    #[test]
    fn dkim_sign_multiple_recipients() {
        let cfg = test_config();
        let msg = b"From: sender@example.com\r\n\
                     To: alice@example.com, bob@example.com,\r\n\
                      charlie@example.com\r\n\
                     Subject: Group mail\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <multi@example.com>\r\n\
                     \r\n\
                     body\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing with multiple To recipients must succeed");
    }

    // --- different domain/selector ---

    #[test]
    fn dkim_sign_custom_domain_and_selector() {
        let cfg = DkimSignConfig {
            selector: "mail2025".into(),
            domain: "custom-mail.example.org".into(),
            private_key_pem: TEST_RSA_KEY.into(),
        };
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        let signed_str = String::from_utf8_lossy(&signed);
        assert!(signed_str.contains("d=custom-mail.example.org"));
        assert!(signed_str.contains("s=mail2025"));
    }

    // --- config clone ---

    #[test]
    fn dkim_config_clone_is_independent() {
        let cfg1 = test_config();
        let cfg2 = cfg1.clone();
        // both produce valid signatures
        let msg = simple_message();
        let signed1 = cfg1.sign(&msg).unwrap();
        let signed2 = cfg2.sign(&msg).unwrap();
        // body hashes are identical (same key, same message)
        assert_eq!(
            signed1.len(),
            signed2.len(),
            "cloned config must produce identical output"
        );
    }

    // --- multipart mime ---

    #[test]
    fn dkim_sign_multipart_mime_message() {
        let cfg = test_config();
        let msg = b"From: sender@example.com\r\n\
                     To: recipient@example.com\r\n\
                     Subject: Multipart test\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <multipart@example.com>\r\n\
                     MIME-Version: 1.0\r\n\
                     Content-Type: multipart/alternative; boundary=\"boundary42\"\r\n\
                     \r\n\
                     --boundary42\r\n\
                     Content-Type: text/plain; charset=utf-8\r\n\
                     \r\n\
                     Plain text body\r\n\
                     --boundary42\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     \r\n\
                     <html><body><p>HTML body</p></body></html>\r\n\
                     --boundary42--\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing a multipart MIME message must succeed");
        let signed = result.unwrap();
        assert!(signed.ends_with(msg.as_slice()));
    }

    // --- body with only whitespace ---

    #[test]
    fn dkim_sign_whitespace_only_body() {
        let cfg = test_config();
        let msg = b"From: sender@example.com\r\n\
                     To: recipient@example.com\r\n\
                     Subject: Whitespace\r\n\
                     Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                     Message-ID: <ws@example.com>\r\n\
                     \r\n\
                     \r\n   \r\n\t\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing whitespace-only body must succeed");
    }

    // --- bare minimum message ---

    #[test]
    fn dkim_sign_minimal_message() {
        let cfg = test_config();
        // only From header and empty body
        let msg = b"From: x@example.com\r\n\r\n";
        let result = cfg.sign(msg);
        assert!(result.is_ok(), "signing a minimal message must succeed");
    }
}
