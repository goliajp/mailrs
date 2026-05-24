use std::collections::HashMap;
use std::io;
use std::path::Path;

use argon2::Argon2;
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand_core::OsRng;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct UsersFile {
    #[serde(default)]
    users: HashMap<String, UserEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct UserEntry {
    // new format: argon2 hash
    password_hash: Option<String>,
    // legacy format: plaintext
    password: Option<String>,
}

#[derive(Debug, Clone)]
enum StoredPassword {
    Hash(String),
    Plain(String),
}

#[derive(Debug, Clone)]
pub struct UserStore {
    users: HashMap<String, StoredPassword>,
}

impl UserStore {
    pub fn empty() -> Self {
        Self {
            users: HashMap::new(),
        }
    }

    /// create a user store from plaintext username/password pairs (for testing)
    #[cfg(test)]
    pub fn from_plain_passwords(pairs: Vec<(String, String)>) -> Self {
        let users = pairs
            .into_iter()
            .map(|(k, v)| (k, StoredPassword::Plain(v)))
            .collect();
        Self { users }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let file: UsersFile =
            toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let users = file
            .users
            .into_iter()
            .filter_map(|(k, v)| {
                let stored = if let Some(hash) = v.password_hash {
                    StoredPassword::Hash(hash)
                } else if let Some(plain) = v.password {
                    StoredPassword::Plain(plain)
                } else {
                    return None;
                };
                Some((k, stored))
            })
            .collect();

        Ok(Self { users })
    }

    pub fn verify(&self, username: &str, password: &str) -> bool {
        match self.users.get(username) {
            Some(StoredPassword::Hash(hash)) => verify_argon2(password, hash),
            Some(StoredPassword::Plain(plain)) => plain == password,
            None => false,
        }
    }

    /// verify a password against an argon2 hash (static, no UserStore instance needed)
    pub fn verify_hash(password: &str, hash: &str) -> bool {
        verify_argon2(password, hash)
    }

    /// hash a password with argon2id
    pub fn hash_password(password: &str) -> Result<String, password_hash::Error> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2.hash_password(password.as_bytes(), &salt)?;
        Ok(hash.to_string())
    }
}

/// validate email address format
pub fn validate_email(email: &str) -> Result<(), &'static str> {
    if email.len() > 255 { return Err("email too long"); }
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 { return Err("email must contain exactly one @"); }
    if parts[0].is_empty() { return Err("local part cannot be empty"); }
    if parts[1].is_empty() || !parts[1].contains('.') { return Err("invalid domain"); }
    Ok(())
}

/// validate password meets minimum complexity requirements
pub fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.len() < 8 {
        return Err("password must be at least 8 characters");
    }
    Ok(())
}

/// verify a password against an argon2 hash
fn verify_argon2(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// pre-computed dummy hash for constant-time rejection of non-existent users.
/// prevents timing side-channel that reveals whether an account exists.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// perform a dummy argon2 verification to prevent timing attacks.
/// when a user does not exist, call this to spend roughly the same time
/// as a real password verification would, then return false.
pub fn dummy_verify(password: &str) {
    let _ = verify_argon2(password, DUMMY_HASH);
}

#[cfg(test)]
#[path = "users_tests.rs"]
mod tests;

