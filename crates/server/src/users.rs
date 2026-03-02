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
    #[allow(dead_code)]
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

/// verify a password against an argon2 hash
fn verify_argon2(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let hash = UserStore::hash_password("test123").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_argon2("test123", &hash));
        assert!(!verify_argon2("wrong", &hash));
    }

    #[test]
    fn verify_plain_legacy() {
        let mut users = HashMap::new();
        users.insert(
            "alice".into(),
            StoredPassword::Plain("secret".into()),
        );
        let store = UserStore { users };
        assert!(store.verify("alice", "secret"));
        assert!(!store.verify("alice", "wrong"));
    }

    #[test]
    fn verify_wrong_password() {
        let hash = UserStore::hash_password("correct").unwrap();
        let mut users = HashMap::new();
        users.insert("bob".into(), StoredPassword::Hash(hash));
        let store = UserStore { users };
        assert!(store.verify("bob", "correct"));
        assert!(!store.verify("bob", "incorrect"));
    }

    #[test]
    fn hash_uniqueness() {
        let h1 = UserStore::hash_password("same_password").unwrap();
        let h2 = UserStore::hash_password("same_password").unwrap();
        // different salts produce different hashes
        assert_ne!(h1, h2);
        // but both verify
        assert!(verify_argon2("same_password", &h1));
        assert!(verify_argon2("same_password", &h2));
    }

    #[test]
    fn verify_nonexistent_user() {
        let store = UserStore::empty();
        assert!(!store.verify("nobody", "password"));
    }
}
