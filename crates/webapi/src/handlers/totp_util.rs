//! TOTP helpers — mirrored from `crates/server/src/totp.rs` so
//! `mailrs-webapi` doesn't need to depend on the server binary crate.
//! Same algorithm (SHA1, 6 digits, 30 s period).

use axum::http::StatusCode;
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

/// Field of `totp:{address}` holding the recovery codes' argon2 hashes,
/// one per line (a PHC string has commas in it).
pub const RECOVERY_HASHES: &[u8] = b"recovery_hashes";
/// Where codes were once kept in plaintext; read only to convert them.
const RECOVERY_PLAIN: &[u8] = b"recovery_codes";

fn hash(code: &str) -> Result<String, password_hash::Error> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2::Argon2::default()
        .hash_password(code.as_bytes(), &salt)?
        .to_string())
}

fn matches(code: &str, stored: &str) -> bool {
    use password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(stored).is_ok_and(|h| {
        argon2::Argon2::default()
            .verify_password(code.as_bytes(), &h)
            .is_ok()
    })
}

/// The stored form of `codes`.
pub fn hash_recovery_codes(codes: &[String]) -> String {
    codes
        .iter()
        .filter_map(|c| hash(c).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where an account stands on its second factor for one attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum SecondFactor {
    /// No authenticator enrolled; the password is all there is.
    NotEnrolled,
    /// Enrolled, and the attempt carried no code.
    Missing,
    /// The code is the current one-time code, or a recovery code (now used up).
    Passed,
    /// The code is neither.
    Failed,
}

/// Check `code` against `address`'s enrolled authenticator, the way every
/// sign-in and every confirmation does. A store that cannot be read is an
/// error, never "not enrolled": that would let a blink of the store skip
/// the second factor.
pub fn second_factor(address: &str, code: Option<&str>) -> Result<SecondFactor, StatusCode> {
    let key = mailrs_core_sidestate::keys::credentials::totp_key(address);
    let k = key.clone();
    let (secret, enabled) = crate::handlers::kevy_util::with_kevy(move |c| {
        Ok((
            c.hget(k.as_bytes(), b"secret")?,
            c.hget(k.as_bytes(), b"enabled")?,
        ))
    })?;
    let (Some(secret), Some(b"1")) = (secret, enabled.as_deref()) else {
        return Ok(SecondFactor::NotEnrolled);
    };
    let Some(code) = code else {
        return Ok(SecondFactor::Missing);
    };
    if verify_code(&String::from_utf8(secret).unwrap_or_default(), code) {
        return Ok(SecondFactor::Passed);
    }
    // one-shot: a recovery code that matches is removed in the same call
    let code = code.to_string();
    let recovered =
        crate::handlers::kevy_util::with_kevy(move |c| consume_recovery_code(c, &key, &code))?;
    if recovered {
        return Ok(SecondFactor::Passed);
    }
    Ok(SecondFactor::Failed)
}

/// Use up `code` if it is one of `key`'s recovery codes. Codes still kept
/// in plaintext are hashed and the plaintext dropped on the way, whether
/// or not `code` matches.
pub fn consume_recovery_code(
    c: &mut kevy_client::Connection,
    key: &str,
    code: &str,
) -> std::io::Result<bool> {
    let read = |c: &mut kevy_client::Connection, field: &[u8]| -> std::io::Result<String> {
        Ok(c.hget(key.as_bytes(), field)?
            .and_then(|v| String::from_utf8(v).ok())
            .unwrap_or_default())
    };
    let mut hashes: Vec<String> = read(c, RECOVERY_HASHES)?
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let plain = read(c, RECOVERY_PLAIN)?;
    let legacy: Vec<String> = plain
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let converted = !legacy.is_empty();
    hashes.extend(hash_recovery_codes(&legacy).lines().map(str::to_string));
    let code = code.trim();
    let hit = (!code.is_empty())
        .then(|| hashes.iter().position(|h| matches(code, h)))
        .flatten();
    if let Some(i) = hit {
        hashes.remove(i);
    }
    if converted || hit.is_some() {
        c.hset(
            key.as_bytes(),
            &[(RECOVERY_HASHES, hashes.join("\n").as_bytes())],
        )?;
    }
    if converted {
        c.hdel(key.as_bytes(), &[RECOVERY_PLAIN])?;
    }
    Ok(hit.is_some())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn a_code_works_once_and_plaintext_codes_are_converted() {
        let mut c = kevy_client::Connection::connect("mem://recovery-codes").unwrap();
        let key = &mailrs_core_sidestate::keys::credentials::totp_key("alice@x.test");
        c.hset(key.as_bytes(), &[(RECOVERY_PLAIN, b"aaaa1111,bbbb2222")])
            .unwrap();
        assert!(!consume_recovery_code(&mut c, key, "nope").unwrap());
        assert_eq!(
            c.hget(key.as_bytes(), RECOVERY_PLAIN).unwrap(),
            None,
            "plaintext dropped"
        );
        let stored =
            String::from_utf8(c.hget(key.as_bytes(), RECOVERY_HASHES).unwrap().unwrap()).unwrap();
        assert_eq!(stored.lines().count(), 2);
        assert!(!stored.contains("aaaa1111"));
        assert!(consume_recovery_code(&mut c, key, "aaaa1111").unwrap());
        assert!(
            !consume_recovery_code(&mut c, key, "aaaa1111").unwrap(),
            "used up"
        );
        assert!(consume_recovery_code(&mut c, key, " bbbb2222 ").unwrap());
        assert!(!consume_recovery_code(&mut c, key, "").unwrap());
    }

    #[test]
    fn new_codes_are_stored_hashed() {
        let codes = generate_recovery_codes();
        let stored = hash_recovery_codes(&codes);
        assert_eq!(stored.lines().count(), 8);
        for code in &codes {
            assert!(!stored.contains(code.as_str()));
        }
    }
}
