/// rewrite envelope sender using SRS (Sender Rewriting Scheme)
/// format: SRS0=hash=tt=original_domain=local_part@local_domain
pub(super) fn srs_rewrite(sender: &str, local_domain: &str, secret: &str) -> String {
    let Some((local_part, original_domain)) = sender.split_once('@') else {
        return sender.to_string();
    };

    // timestamp tag: days since epoch mod 1024 (10-bit, base32-ish)
    let days = (chrono::Utc::now().timestamp() / 86400) as u32 % 1024;
    let tt = format!("{days:03}");

    // HMAC-SHA1 of the rewritten components
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(tt.as_bytes());
    mac.update(original_domain.as_bytes());
    mac.update(local_part.as_bytes());
    let hash = hex::encode(&mac.finalize().into_bytes()[..4]);

    format!("SRS0={hash}={tt}={original_domain}={local_part}@{local_domain}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srs_rewrite_format() {
        let result = srs_rewrite("user@example.com", "mx.local", "secret123");
        assert!(result.starts_with("SRS0="), "expected SRS0= prefix, got: {result}");
        assert!(result.ends_with("@mx.local"), "expected @mx.local suffix, got: {result}");
        assert!(result.contains("=example.com=user@"), "expected domain and local part, got: {result}");
    }

    #[test]
    fn srs_rewrite_no_at_passthrough() {
        let result = srs_rewrite("postmaster", "mx.local", "secret");
        assert_eq!(result, "postmaster");
    }

    #[test]
    fn srs_rewrite_deterministic_hash() {
        let a = srs_rewrite("test@example.com", "mx.local", "key1");
        let b = srs_rewrite("test@example.com", "mx.local", "key1");
        assert_eq!(a, b, "same inputs should produce same output");
    }

    #[test]
    fn srs_rewrite_different_secrets() {
        let a = srs_rewrite("test@example.com", "mx.local", "key1");
        let b = srs_rewrite("test@example.com", "mx.local", "key2");
        assert_ne!(a, b, "different secrets should produce different hashes");
    }
}
