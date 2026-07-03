//! TOTP helpers — mirrored from `crates/server/src/totp.rs` so
//! `mailrs-webapi` doesn't need to depend on the server binary crate.
//! Same algorithm (SHA1, 6 digits, 30 s period).

use rand_core::{OsRng, RngCore};
use totp_rs::{Algorithm, Secret, TOTP};

/// Generate a random base32-encoded TOTP secret (160 bits / 20 bytes).
pub fn generate_secret() -> String {
    let mut raw = [0u8; 20];
    OsRng.fill_bytes(&mut raw);
    let secret = Secret::Raw(raw.to_vec());
    secret.to_encoded().to_string()
}

/// Build a TOTP instance from a base32-encoded secret + account label.
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

/// Verify a 6-digit code against a base32-encoded secret.
pub fn verify_code(secret_base32: &str, code: &str) -> bool {
    let Ok(totp) = build_totp(secret_base32, "") else {
        return false;
    };
    totp.check_current(code).unwrap_or(false)
}

/// otpauth URL for QR-code enrollment.
pub fn get_otpauth_url(secret_base32: &str, account: &str, issuer: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account}?secret={secret_base32}&issuer={issuer}&algorithm=SHA1&digits=6&period=30",
    )
}

/// Eight 8-hex-char recovery codes.
pub fn generate_recovery_codes() -> Vec<String> {
    (0..8)
        .map(|_| {
            let mut bytes = [0u8; 4];
            OsRng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        })
        .collect()
}
