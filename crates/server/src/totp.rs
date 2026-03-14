use rand_core::{OsRng, RngCore};
use totp_rs::{Algorithm, Secret, TOTP};

/// generate a random base32-encoded TOTP secret (160 bits / 20 bytes)
pub fn generate_secret() -> String {
    let mut raw = [0u8; 20];
    OsRng.fill_bytes(&mut raw);
    let secret = Secret::Raw(raw.to_vec());
    secret.to_encoded().to_string()
}

/// build a TOTP instance from a base32-encoded secret
pub fn build_totp(secret_base32: &str, account: &str) -> Result<TOTP, String> {
    let bytes = Secret::Encoded(secret_base32.to_string())
        .to_bytes()
        .map_err(|e| e.to_string())?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some("mailrs".to_string()),
        account.to_string(),
    )
    .map_err(|e| e.to_string())
}

/// verify a 6-digit TOTP code against a base32-encoded secret
pub fn verify_code(secret_base32: &str, code: &str) -> bool {
    let Ok(totp) = build_totp(secret_base32, "") else {
        return false;
    };
    totp.check_current(code).unwrap_or(false)
}

/// generate an otpauth:// URL for QR code scanning
pub fn get_otpauth_url(secret_base32: &str, account: &str, issuer: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account}?secret={secret_base32}&issuer={issuer}&algorithm=SHA1&digits=6&period=30",
    )
}

/// generate 8 random recovery codes (8-char hex each)
pub fn generate_recovery_codes() -> Vec<String> {
    (0..8)
        .map(|_| {
            let mut bytes = [0u8; 4];
            OsRng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_secret_returns_valid_base32() {
        let secret = generate_secret();
        assert!(!secret.is_empty());
        assert!(Secret::Encoded(secret).to_bytes().is_ok());
    }

    #[test]
    fn build_totp_with_valid_secret() {
        let secret = generate_secret();
        assert!(build_totp(&secret, "test@example.com").is_ok());
    }

    #[test]
    fn build_totp_with_invalid_secret() {
        assert!(build_totp("!!!invalid!!!", "test@example.com").is_err());
    }

    #[test]
    fn verify_code_rejects_wrong_code() {
        let secret = generate_secret();
        assert!(!verify_code(&secret, "000000"));
    }

    #[test]
    fn verify_code_accepts_current_code() {
        let secret = generate_secret();
        let totp = build_totp(&secret, "test@example.com").unwrap();
        let code = totp.generate_current().unwrap();
        assert!(verify_code(&secret, &code));
    }

    #[test]
    fn otpauth_url_format() {
        let secret = generate_secret();
        let url = get_otpauth_url(&secret, "user@example.com", "mailrs");
        assert!(url.starts_with("otpauth://totp/mailrs:user@example.com?"));
        assert!(url.contains(&format!("secret={secret}")));
        assert!(url.contains("issuer=mailrs"));
    }

    #[test]
    fn recovery_codes_count_and_length() {
        let codes = generate_recovery_codes();
        assert_eq!(codes.len(), 8);
        for code in &codes {
            assert_eq!(code.len(), 8);
            assert!(code.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }
}
