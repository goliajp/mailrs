//! Outbound DKIM signing + ARC sealing, on the mailrs-* crates.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use mailrs_arc::{
    ArcChain, ArcSealCv, ArcSigningKey, Canon as ArcCanon, ChainOutcome, SealOpts,
    seal as arc_seal, verify_chain_with_crypto,
};
use mailrs_dkim::{
    Canon as DkimCanon, DkimSigningKey, HickoryDkimResolver, RsaSigningKey, SignOpts,
    sign as dkim_sign, verify_all,
};

/// One DKIM key bound to a single signing domain — used as a value
/// in [`DkimSignConfig::extra_keys`] for multi-domain signing.
///
/// The PKCS#8 PEM is parsed into an `RsaSigningKey` lazily on first
/// use and cached for the lifetime of this entry, same as the
/// default key on `DkimSignConfig`.
#[derive(Debug, Clone, Default)]
pub struct DkimDomainKey {
    /// DKIM selector — the label under `<selector>._domainkey.<this domain>`.
    pub selector: String,
    /// Private RSA key in PKCS#8 PEM form.
    pub private_key_pem: String,
    /// Lazy-parsed `RsaSigningKey`, shared across clones.
    #[doc(hidden)]
    pub parsed_key: Arc<OnceLock<Result<RsaSigningKey, String>>>,
}

impl DkimDomainKey {
    fn rsa_key(&self) -> Result<&RsaSigningKey, String> {
        let cached = self
            .parsed_key
            .get_or_init(|| load_rsa_key(&self.private_key_pem));
        match cached {
            Ok(k) => Ok(k),
            Err(e) => Err(e.clone()),
        }
    }
}

/// DKIM signing configuration.
///
/// The PKCS#8 PEM is parsed into an `RsaSigningKey` lazily on first use
/// and cached for the lifetime of this config — important on the hot
/// outbound path where every delivered message triggers a sign call.
/// Existing struct-literal callers need to add `..Default::default()`
/// (the parsed-key cache is non-public, populated on demand).
///
/// **v3 (mailrs-dkim 3.0)**: `RsaSigningKey` wraps aws-lc-rs's
/// `RsaKeyPair`, so per-sign cost dropped from ~1.5 ms (pure-Rust
/// `rsa` crate) to ~0.5 ms.
///
/// **v4 (2026-06-03)**: added [`extra_keys`](Self::extra_keys) to
/// support per-domain DKIM signing. [`sign`](Self::sign) parses the
/// message's `From:` header and looks up the matching key by domain
/// (exact match → ancestor-domain suffix walk → default). The
/// pre-v4 single-domain config (just `selector` / `domain` /
/// `private_key_pem`) keeps working unchanged — `extra_keys` empty
/// = old behaviour.
#[derive(Debug, Clone, Default)]
pub struct DkimSignConfig {
    /// DKIM selector — the label under `<selector>._domainkey.<domain>`.
    /// Used as the default when the message's From: domain doesn't
    /// match any `extra_keys` entry.
    pub selector: String,
    /// Signing domain (matches the `d=` tag in the DKIM-Signature header).
    /// Default `d=` when no `extra_keys` entry matches.
    pub domain: String,
    /// Private RSA key in PKCS#8 PEM form.
    pub private_key_pem: String,
    /// Lazy-parsed `RsaSigningKey`, shared across clones so worker
    /// concurrency doesn't re-parse per delivery thread.
    ///
    /// **Implementation detail** — leave at `Default::default()`. Pub
    /// only so out-of-crate struct-literal callers can spread
    /// `..Default::default()`. Reading or mutating this field
    /// directly is not supported and the type may change.
    #[doc(hidden)]
    pub parsed_key: Arc<OnceLock<Result<RsaSigningKey, String>>>,
    /// Extra DKIM keys keyed by signing domain, used when the
    /// message's `From:` header domain matches an entry (exact match
    /// first, then ancestor-suffix walk — e.g. `mail.example.com`
    /// falls back to `example.com`). When no entry matches, signing
    /// uses the default `selector`/`domain`/`private_key_pem` above.
    ///
    /// Keep `extra_keys.is_empty()` for the single-domain config.
    pub extra_keys: HashMap<String, DkimDomainKey>,
}

impl DkimSignConfig {
    /// Return a borrowed handle to the parsed RSA key (default key
    /// only), parsing the PEM once on first call and caching the
    /// result (success OR error) so every later call is a
    /// pointer-load.
    fn rsa_key(&self) -> Result<&RsaSigningKey, String> {
        let cached = self
            .parsed_key
            .get_or_init(|| load_rsa_key(&self.private_key_pem));
        match cached {
            Ok(k) => Ok(k),
            Err(e) => Err(e.clone()),
        }
    }

    /// Look up the (`d=` domain, selector, RSA key) triple for a given
    /// `From:` header domain. Lookup order:
    ///
    /// 1. **Exact match** in [`extra_keys`](Self::extra_keys).
    /// 2. **Ancestor-suffix walk**: strip the leading label one at a
    ///    time and try `extra_keys` (so `mail.example.com` falls back
    ///    to `example.com`, useful for system mail like
    ///    `postmaster@mail.<your-org>` signed with the parent-domain
    ///    key — DMARC alignment under `adkim=r` accepts subdomain →
    ///    parent-domain alignment).
    /// 3. **Default**: the top-level `selector` / `domain` /
    ///    `private_key_pem` on this config.
    ///
    /// Returns `(d_value, selector, parsed_rsa_key)` with `d_value`
    /// being the matched key (or the default `domain` field).
    fn key_for_from_domain(
        &self,
        from_domain: &str,
    ) -> Result<(String, String, RsaSigningKey), String> {
        // 1. exact match
        if let Some(dk) = self.extra_keys.get(from_domain) {
            return Ok((
                from_domain.to_string(),
                dk.selector.clone(),
                dk.rsa_key()?.clone(),
            ));
        }
        // 2. suffix walk — strip the leading label and try again.
        //    The walk stops once there's only one label left (we never
        //    match against a bare TLD).
        let mut rest = from_domain;
        while let Some(idx) = rest.find('.') {
            let parent = &rest[idx + 1..];
            // require at least one '.' remaining (so we don't match
            // against a bare TLD like "com")
            if !parent.contains('.') {
                break;
            }
            if let Some(dk) = self.extra_keys.get(parent) {
                return Ok((
                    parent.to_string(),
                    dk.selector.clone(),
                    dk.rsa_key()?.clone(),
                ));
            }
            rest = parent;
        }
        // 3. default
        Ok((
            self.domain.clone(),
            self.selector.clone(),
            self.rsa_key()?.clone(),
        ))
    }

    /// Sign a message, prepending the DKIM-Signature header.
    ///
    /// Parses the message's `From:` header to determine the signing
    /// domain via [`key_for_from_domain`](Self::key_for_from_domain).
    /// If no `From:` header is present (or it's malformed enough that
    /// no domain can be extracted), falls back to the default
    /// `domain`/`selector`/`key` on this config.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        let from_domain = extract_from_domain(message);
        let (d_value, selector, rsa) = match from_domain.as_deref() {
            Some(d) => self.key_for_from_domain(d)?,
            None => (
                self.domain.clone(),
                self.selector.clone(),
                self.rsa_key()?.clone(),
            ),
        };
        let key = DkimSigningKey::Rsa(rsa);
        let opts = SignOpts {
            domain: d_value,
            selector,
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
        let header =
            dkim_sign(message, &key, &opts).map_err(|e| format!("DKIM signing failed: {e}"))?;
        let mut signed = Vec::with_capacity(header.len() + message.len());
        signed.extend_from_slice(header.as_bytes());
        signed.extend_from_slice(message);
        Ok(signed)
    }
}

/// Extract the domain part of the message's `From:` header, if any.
///
/// Tolerates the three common shapes:
/// - `alice@example.com`
/// - `Alice <alice@example.com>`
/// - `"Display Name" <alice@example.com>`
///
/// Returns `None` if no `From:` header exists, the header value
/// contains no `@`, or the domain segment is empty.
pub fn extract_from_domain(message: &[u8]) -> Option<String> {
    let m = mailrs_rfc5322::Message::new(message);
    let from_value = m.header("From")?;
    let s = std::str::from_utf8(from_value).ok()?;
    let at = s.rfind('@')?;
    let after = &s[at + 1..];
    // Walk forward until a terminator: '>', whitespace, comma, semicolon.
    let end = after
        .find(|c: char| c == '>' || c == ',' || c == ';' || c.is_whitespace())
        .unwrap_or(after.len());
    let domain = after[..end].trim().to_ascii_lowercase();
    if domain.is_empty() {
        None
    } else {
        Some(domain)
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
    let prior_chain =
        ArcChain::extract(message).map_err(|e| format!("ARC chain extract failed: {e}"))?;
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
        signed_headers: [
            "From",
            "To",
            "Subject",
            "Date",
            "Message-ID",
            "DKIM-Signature",
        ]
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

fn load_rsa_key(pem: &str) -> Result<RsaSigningKey, String> {
    RsaSigningKey::from_pkcs8_pem(pem).map_err(|e| format!("failed to parse DKIM key: {e}"))
}

fn build_authres_body(authserv_id: &str, outputs: &[mailrs_dkim::SignatureOutput]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    s.push_str(authserv_id);
    if outputs.is_empty() {
        s.push_str("; dkim=none");
        return s;
    }
    for o in outputs {
        let verdict = if o.is_pass() { "pass" } else { "fail" };
        let d = o.domain();
        // write! avoids the per-iter intermediate String alloc the
        // previous push_str(&format!(...)) pair was paying.
        if d.is_empty() {
            let _ = write!(s, "; dkim={verdict}");
        } else {
            let _ = write!(s, "; dkim={verdict} header.d={d}");
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

    // ── multi-domain signing (v4 2026-06-03) ─────────────────

    #[test]
    fn extract_from_domain_bare_addr() {
        let m = b"From: alice@example.com\r\n\r\nbody";
        assert_eq!(extract_from_domain(m).as_deref(), Some("example.com"));
    }

    #[test]
    fn extract_from_domain_angle_addr() {
        let m = b"From: Alice <alice@example.com>\r\n\r\nbody";
        assert_eq!(extract_from_domain(m).as_deref(), Some("example.com"));
    }

    #[test]
    fn extract_from_domain_quoted_display_name() {
        let m = b"From: \"Alice Liddell\" <alice@mail.example.com>\r\n\r\nbody";
        assert_eq!(extract_from_domain(m).as_deref(), Some("mail.example.com"));
    }

    #[test]
    fn extract_from_domain_lowercases() {
        let m = b"From: <alice@MAIL.EXAMPLE.com>\r\n\r\nbody";
        assert_eq!(extract_from_domain(m).as_deref(), Some("mail.example.com"));
    }

    #[test]
    fn extract_from_domain_no_from_header() {
        let m = b"To: bob@example.com\r\n\r\nbody";
        assert!(extract_from_domain(m).is_none());
    }

    #[test]
    fn extract_from_domain_no_at_sign() {
        let m = b"From: garbage-no-at\r\n\r\nbody";
        assert!(extract_from_domain(m).is_none());
    }

    #[test]
    fn key_lookup_exact_match() {
        let mut extra = HashMap::new();
        extra.insert(
            "other.com".to_string(),
            DkimDomainKey {
                selector: "k1".into(),
                private_key_pem: TEST_RSA_KEY.into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "default".into(),
            domain: "example.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            extra_keys: extra,
            ..Default::default()
        };
        let (d, sel, _) = cfg.key_for_from_domain("other.com").unwrap();
        assert_eq!(d, "other.com");
        assert_eq!(sel, "k1");
    }

    #[test]
    fn key_lookup_suffix_walk() {
        // From domain `mail.other.com` should resolve to the `other.com`
        // entry via ancestor-suffix walk (strip leading `mail.` label).
        let mut extra = HashMap::new();
        extra.insert(
            "other.com".to_string(),
            DkimDomainKey {
                selector: "suffix".into(),
                private_key_pem: TEST_RSA_KEY.into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "default".into(),
            domain: "example.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            extra_keys: extra,
            ..Default::default()
        };
        let (d, sel, _) = cfg.key_for_from_domain("mail.other.com").unwrap();
        assert_eq!(d, "other.com");
        assert_eq!(sel, "suffix");
    }

    #[test]
    fn key_lookup_suffix_walk_two_levels() {
        // a.b.org → b.org (still keep the entry).
        let mut extra = HashMap::new();
        extra.insert(
            "b.org".to_string(),
            DkimDomainKey {
                selector: "borg".into(),
                private_key_pem: TEST_RSA_KEY.into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "default".into(),
            domain: "example.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            extra_keys: extra,
            ..Default::default()
        };
        let (d, sel, _) = cfg.key_for_from_domain("a.b.org").unwrap();
        assert_eq!(d, "b.org");
        assert_eq!(sel, "borg");
    }

    #[test]
    fn key_lookup_default_fallback() {
        // No extra-key match, even via suffix walk → use the default
        // selector + domain + key.
        let mut extra = HashMap::new();
        extra.insert(
            "other.com".to_string(),
            DkimDomainKey {
                selector: "other".into(),
                private_key_pem: TEST_RSA_KEY.into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "default".into(),
            domain: "example.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            extra_keys: extra,
            ..Default::default()
        };
        let (d, sel, _) = cfg.key_for_from_domain("unrelated.net").unwrap();
        assert_eq!(d, "example.com");
        assert_eq!(sel, "default");
    }

    #[test]
    fn key_lookup_does_not_match_bare_tld() {
        // Even if someone configures `extra_keys["com"]`, a From of
        // `alice@something.com` must not pick it up via suffix walk —
        // signing with d=com would be nonsense. The walk stops at a
        // dot-less parent.
        let mut extra = HashMap::new();
        extra.insert(
            "com".to_string(),
            DkimDomainKey {
                selector: "comkey".into(),
                private_key_pem: TEST_RSA_KEY.into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "default".into(),
            domain: "example.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            extra_keys: extra,
            ..Default::default()
        };
        let (d, sel, _) = cfg.key_for_from_domain("something.com").unwrap();
        // suffix walk stops because `com` has no dot — fell back to default
        assert_eq!(d, "example.com");
        assert_eq!(sel, "default");
    }

    #[test]
    fn sign_picks_d_from_from_header_via_extra_keys() {
        // The point of the v4 multi-domain refactor: a message with
        // `From: postmaster@mail.example.com` must end up signed with
        // `d=example.com` when `extra_keys["example.com"]` exists,
        // not with the default `d=otherdefault.com`.
        let mut extra = HashMap::new();
        extra.insert(
            "example.com".to_string(),
            DkimDomainKey {
                selector: "mail".into(),
                private_key_pem: TEST_RSA_KEY.into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "default".into(),
            domain: "otherdefault.com".into(),
            private_key_pem: TEST_RSA_KEY.into(),
            extra_keys: extra,
            ..Default::default()
        };
        let msg = b"From: postmaster@mail.example.com\r\n\
                    To: r@example.com\r\n\
                    Subject: hi\r\n\
                    Date: Thu, 01 Jan 2026 00:00:00 +0000\r\n\
                    Message-ID: <abc@mail.example.com>\r\n\
                    \r\n\
                    body\r\n";
        let signed = cfg.sign(msg).unwrap();
        let s = String::from_utf8_lossy(&signed);
        assert!(
            s.contains("d=example.com"),
            "expected d=example.com (via suffix-walk lookup), got {}",
            s.lines().next().unwrap_or("")
        );
        assert!(
            !s.contains("d=otherdefault.com"),
            "default domain should NOT have been used"
        );
        assert!(s.contains("s=mail"));
    }
}
