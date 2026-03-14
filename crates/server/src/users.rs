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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// helper: build a UserStore from a TOML string, mirroring UserStore::load logic
    fn store_from_toml(content: &str) -> Result<UserStore, String> {
        let file: UsersFile =
            toml::from_str(content).map_err(|e| e.to_string())?;
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
        Ok(UserStore { users })
    }

    // ── UserStore::empty ──

    #[test]
    fn empty_store_has_no_users() {
        let store = UserStore::empty();
        assert!(!store.verify("", ""));
    }

    #[test]
    fn empty_store_rejects_any_username() {
        let store = UserStore::empty();
        assert!(!store.verify("admin", "admin"));
        assert!(!store.verify("root", "toor"));
        assert!(!store.verify("nobody", "password"));
    }

    // ── UserStore::from_plain_passwords ──

    #[test]
    fn from_plain_passwords_basic() {
        let store = UserStore::from_plain_passwords(vec![
            ("alice".into(), "pass1".into()),
            ("bob".into(), "pass2".into()),
        ]);
        assert!(store.verify("alice", "pass1"));
        assert!(store.verify("bob", "pass2"));
        assert!(!store.verify("alice", "pass2"));
        assert!(!store.verify("bob", "pass1"));
    }

    #[test]
    fn from_plain_passwords_empty_vec() {
        let store = UserStore::from_plain_passwords(vec![]);
        assert!(!store.verify("anyone", "anything"));
    }

    #[test]
    fn from_plain_passwords_single_user() {
        let store =
            UserStore::from_plain_passwords(vec![("solo".into(), "pw".into())]);
        assert!(store.verify("solo", "pw"));
        assert!(!store.verify("solo", "wrong"));
        assert!(!store.verify("other", "pw"));
    }

    #[test]
    fn from_plain_passwords_empty_username_and_password() {
        let store = UserStore::from_plain_passwords(vec![
            ("".into(), "".into()),
        ]);
        assert!(store.verify("", ""));
        assert!(!store.verify("", "notempty"));
    }

    // ── password hashing ──

    #[test]
    fn hash_and_verify() {
        let hash = UserStore::hash_password("test123").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_argon2("test123", &hash));
        assert!(!verify_argon2("wrong", &hash));
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
    fn hash_password_empty_string() {
        let hash = UserStore::hash_password("").unwrap();
        assert!(verify_argon2("", &hash));
        assert!(!verify_argon2(" ", &hash));
    }

    #[test]
    fn hash_password_long_input() {
        let long_pw = "a".repeat(1024);
        let hash = UserStore::hash_password(&long_pw).unwrap();
        assert!(verify_argon2(&long_pw, &hash));
        assert!(!verify_argon2(&"a".repeat(1023), &hash));
    }

    #[test]
    fn hash_password_unicode() {
        let hash = UserStore::hash_password("密码测试🔑").unwrap();
        assert!(verify_argon2("密码测试🔑", &hash));
        assert!(!verify_argon2("密码测试", &hash));
    }

    #[test]
    fn hash_password_special_chars() {
        let pw = r#"p@$$w0rd!#%^&*(){}[]|\"'<>,.?/~`"#;
        let hash = UserStore::hash_password(pw).unwrap();
        assert!(verify_argon2(pw, &hash));
    }

    // ── verify_argon2 edge cases ──

    #[test]
    fn verify_invalid_hash_returns_false() {
        assert!(!verify_argon2("any", "not-a-valid-hash"));
        assert!(!verify_argon2("any", ""));
    }

    #[test]
    fn verify_argon2_malformed_prefix() {
        assert!(!verify_argon2("pw", "$argon2id$"));
        assert!(!verify_argon2("pw", "$argon2id$v=19$"));
        assert!(!verify_argon2("pw", "$argon2id$v=19$m=19456,t=2,p=1$"));
    }

    #[test]
    fn verify_argon2_wrong_algorithm_tag() {
        // a valid-looking structure but with nonsense algorithm
        assert!(!verify_argon2("pw", "$bcrypt$v=19$m=19456,t=2,p=1$salt$hash"));
    }

    // ── UserStore::verify with plain passwords ──

    #[test]
    fn verify_plain_legacy() {
        let mut users = HashMap::new();
        users.insert("alice".into(), StoredPassword::Plain("secret".into()));
        let store = UserStore { users };
        assert!(store.verify("alice", "secret"));
        assert!(!store.verify("alice", "wrong"));
    }

    #[test]
    fn verify_plain_case_sensitive() {
        let store = UserStore::from_plain_passwords(vec![
            ("alice".into(), "Secret".into()),
        ]);
        assert!(store.verify("alice", "Secret"));
        assert!(!store.verify("alice", "secret"));
        assert!(!store.verify("alice", "SECRET"));
    }

    #[test]
    fn verify_username_case_sensitive() {
        let store = UserStore::from_plain_passwords(vec![
            ("Alice".into(), "pw".into()),
        ]);
        assert!(store.verify("Alice", "pw"));
        assert!(!store.verify("alice", "pw"));
        assert!(!store.verify("ALICE", "pw"));
    }

    // ── UserStore::verify with argon2 hashes ──

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
    fn verify_hash_user_not_found() {
        let hash = UserStore::hash_password("pw").unwrap();
        let mut users = HashMap::new();
        users.insert("exists".into(), StoredPassword::Hash(hash));
        let store = UserStore { users };
        assert!(!store.verify("missing", "pw"));
    }

    // ── UserStore::verify_hash (static method) ──

    #[test]
    fn verify_hash_static() {
        let hash = UserStore::hash_password("mypass").unwrap();
        assert!(UserStore::verify_hash("mypass", &hash));
        assert!(!UserStore::verify_hash("notmypass", &hash));
    }

    #[test]
    fn verify_hash_static_invalid() {
        assert!(!UserStore::verify_hash("pw", "garbage"));
        assert!(!UserStore::verify_hash("pw", ""));
    }

    // ── TOML parsing: from_toml_content (via helper) ──

    #[test]
    fn parse_toml_plaintext_password() {
        let toml_str = r#"
[users.alice]
password = "plaintext_secret"
"#;
        let store = store_from_toml(toml_str).unwrap();
        assert!(store.verify("alice", "plaintext_secret"));
        assert!(!store.verify("alice", "wrong"));
    }

    #[test]
    fn parse_toml_argon2_hash() {
        let hash = UserStore::hash_password("hashed_pw").unwrap();
        let toml_str = format!(
            r#"
[users.bob]
password_hash = "{hash}"
"#
        );
        let store = store_from_toml(&toml_str).unwrap();
        assert!(store.verify("bob", "hashed_pw"));
        assert!(!store.verify("bob", "wrong"));
    }

    #[test]
    fn parse_toml_mixed_password_types() {
        let hash = UserStore::hash_password("hash_pw").unwrap();
        let toml_str = format!(
            r#"
[users.plain_user]
password = "my_plain_pw"

[users.hash_user]
password_hash = "{hash}"
"#
        );
        let store = store_from_toml(&toml_str).unwrap();
        assert!(store.verify("plain_user", "my_plain_pw"));
        assert!(store.verify("hash_user", "hash_pw"));
        assert!(!store.verify("plain_user", "hash_pw"));
        assert!(!store.verify("hash_user", "my_plain_pw"));
    }

    #[test]
    fn parse_toml_password_hash_takes_precedence() {
        // when both password and password_hash are present, hash wins
        let hash = UserStore::hash_password("hash_wins").unwrap();
        let toml_str = format!(
            r#"
[users.both]
password = "plain_loses"
password_hash = "{hash}"
"#
        );
        let store = store_from_toml(&toml_str).unwrap();
        assert!(store.verify("both", "hash_wins"));
        assert!(!store.verify("both", "plain_loses"));
    }

    #[test]
    fn parse_toml_empty_file() {
        let store = store_from_toml("").unwrap();
        assert!(!store.verify("anyone", "anything"));
    }

    #[test]
    fn parse_toml_empty_users_section() {
        let store = store_from_toml("[users]\n").unwrap();
        assert!(!store.verify("anyone", "anything"));
    }

    #[test]
    fn parse_toml_user_without_password_fields_is_skipped() {
        let toml_str = r#"
[users.ghost]
# no password or password_hash
"#;
        let store = store_from_toml(toml_str).unwrap();
        assert!(!store.verify("ghost", ""));
        assert!(!store.verify("ghost", "anything"));
    }

    #[test]
    fn parse_toml_invalid_format() {
        let result = store_from_toml("this is not valid toml {{{}}}");
        assert!(result.is_err());
    }

    #[test]
    fn parse_toml_wrong_structure() {
        // valid toml, but wrong structure (users should be a table of tables)
        let result = store_from_toml(r#"users = "not a table""#);
        assert!(result.is_err());
    }

    #[test]
    fn parse_toml_extra_fields_ignored() {
        let toml_str = r#"
[users.alice]
password = "pw"
email = "alice@example.com"
display_name = "Alice"
"#;
        // serde should ignore unknown fields by default with deny_unknown_fields absent
        let store = store_from_toml(toml_str).unwrap();
        assert!(store.verify("alice", "pw"));
    }

    #[test]
    fn parse_toml_multiple_users() {
        let toml_str = r#"
[users.user1]
password = "pw1"

[users.user2]
password = "pw2"

[users.user3]
password = "pw3"

[users.user4]
password = "pw4"

[users.user5]
password = "pw5"
"#;
        let store = store_from_toml(toml_str).unwrap();
        for i in 1..=5 {
            let name = format!("user{i}");
            let pw = format!("pw{i}");
            assert!(store.verify(&name, &pw), "user {name} should verify");
            assert!(
                !store.verify(&name, "wrong"),
                "user {name} should reject wrong pw"
            );
        }
        assert!(!store.verify("user6", "pw6"));
    }

    #[test]
    fn parse_toml_username_with_special_chars() {
        let toml_str = r#"
[users."user@domain.com"]
password = "emailpw"

[users."user-with-dashes"]
password = "dashpw"

[users."user.with.dots"]
password = "dotpw"
"#;
        let store = store_from_toml(toml_str).unwrap();
        assert!(store.verify("user@domain.com", "emailpw"));
        assert!(store.verify("user-with-dashes", "dashpw"));
        assert!(store.verify("user.with.dots", "dotpw"));
    }

    #[test]
    fn parse_toml_unicode_password() {
        let toml_str = r#"
[users.intl]
password = "密码パスワード🔐"
"#;
        let store = store_from_toml(toml_str).unwrap();
        assert!(store.verify("intl", "密码パスワード🔐"));
        assert!(!store.verify("intl", "密码パスワード"));
    }

    #[test]
    fn parse_toml_empty_password_string() {
        let toml_str = r#"
[users.blank]
password = ""
"#;
        let store = store_from_toml(toml_str).unwrap();
        assert!(store.verify("blank", ""));
        assert!(!store.verify("blank", "notempty"));
    }

    // ── UserStore::load (file-based) ──

    #[test]
    fn load_from_valid_file() {
        let dir = std::env::temp_dir().join("mailrs_test_load_valid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("users.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[users.fileuser]
password = "filepw"
"#
        )
        .unwrap();

        let store = UserStore::load(&path).unwrap();
        assert!(store.verify("fileuser", "filepw"));
        assert!(!store.verify("fileuser", "wrong"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_nonexistent_file() {
        let result = UserStore::load(Path::new("/tmp/mailrs_nonexistent_users.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn load_from_empty_file() {
        let dir = std::env::temp_dir().join("mailrs_test_load_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("users.toml");
        std::fs::File::create(&path).unwrap();

        let store = UserStore::load(&path).unwrap();
        assert!(!store.verify("anyone", "anything"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_invalid_toml_file() {
        let dir = std::env::temp_dir().join("mailrs_test_load_invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("users.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{{{{ not valid toml").unwrap();

        let result = UserStore::load(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_file_with_argon2_hash() {
        let hash = UserStore::hash_password("file_hash_pw").unwrap();
        let dir = std::env::temp_dir().join("mailrs_test_load_hash");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("users.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[users.hashuser]
password_hash = "{hash}"
"#
        )
        .unwrap();

        let store = UserStore::load(&path).unwrap();
        assert!(store.verify("hashuser", "file_hash_pw"));
        assert!(!store.verify("hashuser", "wrong"));

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── multi-user scenarios ──

    #[test]
    fn mixed_store_plain_and_hash() {
        let hash = UserStore::hash_password("hpw").unwrap();
        let mut users = HashMap::new();
        users.insert("plain".into(), StoredPassword::Plain("ppw".into()));
        users.insert("hashed".into(), StoredPassword::Hash(hash));
        let store = UserStore { users };

        assert!(store.verify("plain", "ppw"));
        assert!(store.verify("hashed", "hpw"));
        assert!(!store.verify("plain", "hpw"));
        assert!(!store.verify("hashed", "ppw"));
        assert!(!store.verify("unknown", "ppw"));
    }

    #[test]
    fn verify_does_not_mutate_store() {
        let store = UserStore::from_plain_passwords(vec![
            ("a".into(), "1".into()),
        ]);
        // calling verify multiple times should be idempotent
        assert!(store.verify("a", "1"));
        assert!(store.verify("a", "1"));
        assert!(!store.verify("a", "2"));
        assert!(store.verify("a", "1"));
    }

    #[test]
    fn clone_store_is_independent() {
        let store = UserStore::from_plain_passwords(vec![
            ("user".into(), "pw".into()),
        ]);
        let cloned = store.clone();
        assert!(cloned.verify("user", "pw"));
        assert!(!cloned.verify("user", "wrong"));
    }

    // ── UsersFile deserialization ──

    #[test]
    fn users_file_deserialize_both_fields() {
        let toml_str = r#"
[users.alice]
password = "plaintext_secret"

[users.bob]
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$fakesalt$fakehash"
"#;
        let file: UsersFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.users.len(), 2);
        assert!(file.users["alice"].password.is_some());
        assert!(file.users["alice"].password_hash.is_none());
        assert!(file.users["bob"].password_hash.is_some());
        assert!(file.users["bob"].password.is_none());
    }

    #[test]
    fn users_file_deserialize_no_users_section() {
        let file: UsersFile = toml::from_str("").unwrap();
        assert!(file.users.is_empty());
    }

    #[test]
    fn users_file_deserialize_empty_users() {
        let file: UsersFile = toml::from_str("[users]\n").unwrap();
        assert!(file.users.is_empty());
    }

    #[test]
    fn users_file_deserialize_user_no_fields() {
        let toml_str = r#"
[users.minimal]
"#;
        let file: UsersFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.users.len(), 1);
        let entry = &file.users["minimal"];
        assert!(entry.password.is_none());
        assert!(entry.password_hash.is_none());
    }

    // ── whitespace and edge cases in passwords ──

    #[test]
    fn password_with_leading_trailing_spaces() {
        let store = UserStore::from_plain_passwords(vec![
            ("user".into(), "  spaced  ".into()),
        ]);
        assert!(store.verify("user", "  spaced  "));
        assert!(!store.verify("user", "spaced"));
        assert!(!store.verify("user", "  spaced"));
    }

    #[test]
    fn password_with_newlines() {
        let store = UserStore::from_plain_passwords(vec![
            ("user".into(), "line1\nline2".into()),
        ]);
        assert!(store.verify("user", "line1\nline2"));
        assert!(!store.verify("user", "line1line2"));
    }

    #[test]
    fn password_with_null_byte() {
        let store = UserStore::from_plain_passwords(vec![
            ("user".into(), "before\0after".into()),
        ]);
        assert!(store.verify("user", "before\0after"));
        assert!(!store.verify("user", "before"));
    }
}
