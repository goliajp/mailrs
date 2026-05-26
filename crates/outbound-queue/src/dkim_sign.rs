//! Outbound DKIM signing + ARC sealing, on the mailrs-* crates.

use std::sync::{Arc, OnceLock};

use mailrs_arc::{
    ArcChain, ArcSealCv, ArcSigningKey, Canon as ArcCanon, ChainOutcome, SealOpts,
    seal as arc_seal, verify_chain_with_crypto,
};
use mailrs_dkim::{
    Canon as DkimCanon, DkimSigningKey, HickoryDkimResolver, SignOpts, sign as dkim_sign, verify_all,
};
use rsa::RsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;

/// DKIM signing configuration.
///
/// The PKCS#8 PEM is parsed into an `RsaPrivateKey` lazily on first use
/// and cached for the lifetime of this config — important on the hot
/// outbound path where every delivered message triggers a sign call.
/// Existing struct-literal callers need to add `..Default::default()`
/// (the parsed-key cache is non-public, populated on demand).
#[derive(Debug, Clone, Default)]
pub struct DkimSignConfig {
    /// DKIM selector — the label under `<selector>._domainkey.<domain>`.
    pub selector: String,
    /// Signing domain (matches the `d=` tag in the DKIM-Signature header).
    pub domain: String,
    /// Private RSA key in PKCS#8 PEM form.
    pub private_key_pem: String,
    /// Lazy-parsed RsaPrivateKey, shared across clones so worker
    /// concurrency doesn't re-parse per delivery thread.
    ///
    /// **Implementation detail** — leave at `Default::default()`. Pub
    /// only so out-of-crate struct-literal callers can spread
    /// `..Default::default()`. Reading or mutating this field
    /// directly is not supported and the type may change.
    #[doc(hidden)]
    pub parsed_key: Arc<OnceLock<Result<RsaPrivateKey, String>>>,
}

impl DkimSignConfig {
    /// Return a borrowed handle to the parsed RSA key, parsing the PEM
    /// once on first call and caching the result (success OR error) so
    /// every later call is a pointer-load.
    fn rsa_key(&self) -> Result<&RsaPrivateKey, String> {
        let cached = self
            .parsed_key
            .get_or_init(|| load_rsa_key(&self.private_key_pem));
        match cached {
            Ok(k) => Ok(k),
            Err(e) => Err(e.clone()),
        }
    }

    /// Sign a message, prepending the DKIM-Signature header.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        let rsa = self.rsa_key()?;
        let key = DkimSigningKey::Rsa(rsa.clone());
        let opts = SignOpts {
            domain: self.domain.clone(),
            selector: self.selector.clone(),
            signed_headers: ["From", "To", "Subject", "Date", "Message-ID"]
                .iter()
                .map(|&s| s.to_string())
                .collect(),
            canon_header: DkimCanon::Relaxed,
            canon_body: DkimCanon::Relaxed,
            identity: None,
            timestamp: None,
            expiration: None,
            body_length: None,
        };
        let header = dkim_sign(message, &key, &opts)
            .map_err(|e| format!("DKIM signing failed: {e}"))?;
        let mut signed = Vec::with_capacity(header.len() + message.len());
        signed.extend_from_slice(header.as_bytes());
        signed.extend_from_slice(message);
        Ok(signed)
    }
}

/// Extract domain from email address.
pub fn extract_domain(email: &str) -> Option<&str> {
    email.rsplit_once('@').map(|(_, domain)| domain)
}

/// ARC-seal a forwarded message, preserving the authentication chain
/// (RFC 8617). Uses `mailrs-dkim` + `mailrs-arc` for verify-then-seal.
pub async fn arc_seal_message(
    dkim_config: &DkimSignConfig,
    dkim_resolver: &HickoryDkimResolver,
    message: &[u8],
) -> Result<Vec<u8>, String> {
    // 1. Verify existing DKIM signatures — used as our hop's authres body.
    let dkim_outputs = verify_all(dkim_resolver, message).await;

    // 2. Extract + verify the prior ARC chain (if any). `cv=` on the
    //    new seal mirrors the verdict.
    let prior_chain = ArcChain::extract(message)
        .map_err(|e| format!("ARC chain extract failed: {e}"))?;
    let cv = match prior_chain.as_ref() {
        None => ArcSealCv::None,
        Some(chain) => {
            match verify_chain_with_crypto(chain, dkim_resolver, message)
                .await
                .map_err(|e| format!("ARC chain verify failed: {e}"))?
            {
                ChainOutcome::Pass => ArcSealCv::Pass,
                _ => ArcSealCv::Fail,
            }
        }
    };

    // 3. Build the AAR body — coarse DKIM verdict per signature, in
    //    the same shape an `Authentication-Results` header would carry.
    let authres = build_authres_body(&dkim_config.domain, &dkim_outputs);

    // 4. Load the signing key and seal. Reuse the lazy-parsed cache
    //    on DkimSignConfig so we don't re-parse PEM per outbound msg.
    let rsa = dkim_config.rsa_key()?;
    let key = ArcSigningKey::Rsa(rsa);
    let opts = SealOpts {
        domain: dkim_config.domain.clone(),
        selector: dkim_config.selector.clone(),
        signed_headers: ["From", "To", "Subject", "Date", "Message-ID", "DKIM-Signature"]
            .iter()
            .map(|&s| s.to_string())
            .collect(),
        canon_header: ArcCanon::Relaxed,
        canon_body: ArcCanon::Relaxed,
        cv,
        authres,
        timestamp: None,
    };
    let sealed = arc_seal(message, &key, &opts, prior_chain.as_ref())
        .map_err(|e| format!("ARC sealing failed: {e}"))?;

    let prepend = sealed.concat();
    let mut out = Vec::with_capacity(prepend.len() + message.len());
    out.extend_from_slice(prepend.as_bytes());
    out.extend_from_slice(message);
    Ok(out)
}

fn load_rsa_key(pem: &str) -> Result<RsaPrivateKey, String> {
    RsaPrivateKey::from_pkcs8_pem(pem).map_err(|e| format!("failed to parse DKIM key: {e}"))
}

fn build_authres_body(
    authserv_id: &str,
    outputs: &[mailrs_dkim::SignatureOutput],
) -> String {
    let mut s = String::with_capacity(64);
    s.push_str(authserv_id);
    if outputs.is_empty() {
        s.push_str("; dkim=none");
        return s;
    }
    for o in outputs {
        let verdict = if o.is_pass() { "pass" } else { "fail" };
        let d = o.domain();
        if d.is_empty() {
            s.push_str(&format!("; dkim={verdict}"));
        } else {
            s.push_str(&format!("; dkim={verdict} header.d={d}"));
        }
    }
    s
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
            ..Default::default()
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
        assert_eq!(extract_domain("@"), Some(""));
    }

    #[test]
    fn extract_domain_multiple_at_signs_uses_last() {
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
    fn dkim_sign_invalid_pem_returns_error() {
        let cfg = DkimSignConfig {
            selector: "selector".into(),
            domain: "example.com".into(),
            private_key_pem: "not-a-valid-pem".into(),
            ..Default::default()
        };
        let result = cfg.sign(b"From: test@example.com\r\n\r\nbody");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("failed to parse DKIM key"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dkim_sign_empty_pem_returns_error() {
        let cfg = DkimSignConfig {
            selector: "sel".into(),
            domain: "example.com".into(),
            private_key_pem: String::new(),
            ..Default::default()
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
            ..Default::default()
        };
        assert_eq!(cfg.selector, "myselector");
        assert_eq!(cfg.domain, "mydomain.com");
        assert_eq!(cfg.private_key_pem, "pem-data");
    }

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

        let dkim_header = signed_str
            .split_once("From: sender@example.com")
            .expect("original message must follow signature")
            .0;

        assert!(dkim_header.contains("v=1"), "missing v= tag");
        assert!(dkim_header.contains("a=rsa-sha256"), "missing a= tag");
        assert!(dkim_header.contains("d=example.com"), "missing d= tag");
        assert!(dkim_header.contains("s=test"), "missing s= tag");
        assert!(dkim_header.contains("b="), "missing b= tag");
        assert!(dkim_header.contains("bh="), "missing bh= tag");
        assert!(dkim_header.contains("h="), "missing h= tag");
    }

    #[test]
    fn dkim_signature_header_list_contains_signed_fields() {
        let cfg = test_config();
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        let signed_str = String::from_utf8_lossy(&signed);

        let h_start = signed_str.find("h=").expect("h= tag missing");
        let after_h = &signed_str[h_start..];
        let h_value = after_h.split_once(';').map(|(v, _)| v).unwrap_or(after_h);

        let h_lower = h_value.to_lowercase();
        for expected in ["from", "to", "subject", "date", "message-id"] {
            assert!(
                h_lower.contains(expected),
                "h= tag missing expected header: {expected}"
            );
        }
    }

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

    #[test]
    fn dkim_sign_same_input_produces_same_body_hash() {
        let cfg = test_config();
        let msg = simple_message();
        let signed1 = String::from_utf8_lossy(&cfg.sign(&msg).unwrap()).to_string();
        let signed2 = String::from_utf8_lossy(&cfg.sign(&msg).unwrap()).to_string();

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
    fn dkim_sign_large_message() {
        let cfg = test_config();
        let headers = b"From: sender@example.com\r\n\
                         To: recipient@example.com\r\n\
                         Subject: Large message test\r\n\
                         Date: Thu, 01 Jan 2025 00:00:00 +0000\r\n\
                         Message-ID: <large@example.com>\r\n\
                         \r\n";
        let body_line = b"The quick brown fox jumps over the lazy dog. \r\n";
        let repeat_count = 1_000_000 / body_line.len();
        let mut msg = headers.to_vec();
        for _ in 0..repeat_count {
            msg.extend_from_slice(body_line);
        }

        let result = cfg.sign(&msg);
        assert!(result.is_ok(), "signing a ~1MB message must succeed");
    }

    #[test]
    fn dkim_sign_custom_domain_and_selector() {
        let cfg = DkimSignConfig {
            selector: "mail2025".into(),
            domain: "custom-mail.example.org".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            ..Default::default()
        };
        let msg = simple_message();
        let signed = cfg.sign(&msg).unwrap();
        let signed_str = String::from_utf8_lossy(&signed);
        assert!(signed_str.contains("d=custom-mail.example.org"));
        assert!(signed_str.contains("s=mail2025"));
    }

    #[test]
    fn build_authres_empty_outputs() {
        let s = build_authres_body("mx.example.com", &[]);
        assert_eq!(s, "mx.example.com; dkim=none");
    }

    #[test]
    fn build_authres_starts_with_authserv_id() {
        let s = build_authres_body("mx.example.com", &[]);
        assert!(s.starts_with("mx.example.com"));
    }
}
