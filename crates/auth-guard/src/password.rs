//! Passwords: the one check a sign-in makes ([`matches()`]), and what a new
//! password must not be ([`check_new`]). The second is checked whenever a
//! password is set — by its owner, by a reset, by an administrator — and
//! never at sign-in: an existing password keeps working until its owner
//! changes it.

use std::collections::HashSet;
use std::sync::LazyLock;

use argon2::Argon2;
use password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};

/// Whether `password` is the one `hash` was made from: the check every way
/// of signing in makes. Only an argon2 hash can match. An empty one is an
/// account without a password, never one that takes any, and a stored value
/// that is not a hash is never compared as text.
pub fn matches(password: &str, hash: &str) -> bool {
    if password.is_empty() || !hash.starts_with("$argon2") {
        return false;
    }
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// An argon2id hash of `secret`, the form [`matches()`] checks.
///
/// # Errors
/// The hasher refused the input.
pub fn hash(secret: &str) -> Result<String, password_hash::Error> {
    Ok(Argon2::default()
        .hash_password(secret.as_bytes())?
        .to_string())
}

/// What a mail-app sign-in with the account password is told on an
/// account that takes app passwords only: where to make one. The web
/// client is served on the host the server names itself
/// (`MAILRS_HOSTNAME`). ASCII, as protocol responses must be.
pub fn app_only_notice() -> String {
    let settings = std::env::var("MAILRS_HOSTNAME")
        .map(|h| format!("https://{}/settings?tab=security", h.trim()))
        .unwrap_or_else(|_| "Settings > Security".into());
    format!("This account signs in to mail apps with an app password only; make one at {settings}")
}

/// What a sign-in to an account whose password an administrator cleared is
/// told, in every protocol: that the password was reset, and whom to ask for
/// a code to set a new one, from `MAILRS_RESET_CONTACT_NAME` and
/// `MAILRS_RESET_CONTACT`. ASCII only, as protocol responses must be: a
/// name written in another script is left out and the contact kept.
pub fn cleared_notice() -> String {
    format!(
        "Your password was reset by an administrator. Ask {} for a reset code to set a new one",
        contact_text()
    )
}

/// What an account whose outbound mail was paused is told when it sends,
/// in any protocol: whom to ask to release it. The same contact as for a
/// cleared password; ASCII only.
pub fn outbound_paused_notice() -> String {
    format!(
        "Sending from this account is paused after unusual outbound mail. Ask {} to release it",
        contact_text()
    )
}

/// The contact named in these notices, from `MAILRS_RESET_CONTACT_NAME` and
/// `MAILRS_RESET_CONTACT`.
fn contact_text() -> String {
    let read = |name| std::env::var(name).unwrap_or_default();
    who(
        &read("MAILRS_RESET_CONTACT_NAME"),
        &read("MAILRS_RESET_CONTACT"),
    )
}

fn who(name: &str, contact: &str) -> String {
    let ascii = |s: &str| {
        let s = s.trim();
        match s.is_ascii() && !s.chars().any(|c| c.is_ascii_control() || c == '"') {
            true => s.to_string(),
            false => String::new(),
        }
    };
    match (ascii(name), ascii(contact)) {
        (n, c) if !n.is_empty() && !c.is_empty() => format!("{n} <{c}>"),
        (n, c) if n.is_empty() && c.is_empty() => "an administrator".to_string(),
        (n, c) => format!("{n}{c}"),
    }
}

/// Shortest new password.
pub const MIN_LEN: usize = 12;
/// Longest, so a request cannot make hashing arbitrarily slow.
pub const MAX_LEN: usize = 1024;

static COMMON: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    include_str!("common-passwords.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect()
});

/// Why a new password was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weak {
    /// Fewer than [`MIN_LEN`] characters.
    TooShort,
    /// More than [`MAX_LEN`] characters.
    TooLong,
    /// One of the 10,000 most common passwords.
    Common,
}

impl Weak {
    /// The name a client shows its own words for.
    pub fn code(self) -> &'static str {
        match self {
            Weak::TooShort => "password_too_short",
            Weak::TooLong => "password_too_long",
            Weak::Common => "password_common",
        }
    }
}

impl std::fmt::Display for Weak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Weak::TooShort => write!(f, "Use at least {MIN_LEN} characters"),
            Weak::TooLong => write!(f, "Use at most {MAX_LEN} characters"),
            Weak::Common => write!(
                f,
                "This password is one of the most common ones; choose another"
            ),
        }
    }
}

/// Whether `password` may become an account's password. The account's own
/// name and its domain are allowed in it.
pub fn check_new(password: &str) -> Result<(), Weak> {
    let len = password.chars().count();
    if len < MIN_LEN {
        return Err(Weak::TooShort);
    }
    if len > MAX_LEN {
        return Err(Weak::TooLong);
    }
    let lower = password.to_lowercase();
    if COMMON.contains(lower.as_str()) {
        return Err(Weak::Common);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashed(password: &str) -> String {
        use password_hash::PasswordHasher;
        let salt = b"somesaltsomesalt";
        Argon2::default()
            .hash_password_with_salt(password.as_bytes(), salt)
            .expect("hash")
            .to_string()
    }

    #[test]
    fn only_the_password_an_argon2_hash_was_made_from_matches() {
        let hash = hashed("correct horse");
        assert!(matches("correct horse", &hash));
        assert!(!matches("wrong horse", &hash));
        assert!(!matches("", &hash));
    }

    #[test]
    fn an_empty_hash_takes_no_password() {
        assert!(!matches("", ""));
        assert!(!matches("anything", ""));
    }

    #[test]
    fn a_stored_value_that_is_not_a_hash_is_never_compared_as_text() {
        assert!(!matches("hunter2hunter2", "hunter2hunter2"));
        assert!(!matches("$2b$10$abc", "$2b$10$abc"));
    }

    #[test]
    fn an_empty_password_never_matches_even_its_own_hash() {
        assert!(!matches("", &hashed("")));
    }

    #[test]
    fn the_notice_names_whom_to_ask_in_ascii() {
        assert_eq!(who("Li Hao", "lihao@golia.jp"), "Li Hao <lihao@golia.jp>");
        // a name in another script cannot go into a protocol line; the address can
        assert_eq!(who("李浩", "lihao@golia.jp"), "lihao@golia.jp");
        assert_eq!(who("", ""), "an administrator");
        assert_eq!(who("a\"b", "x\r\ny"), "an administrator");
    }

    #[test]
    fn the_list_is_loaded() {
        assert!(COMMON.len() >= 10_000, "{}", COMMON.len());
        assert!(COMMON.contains("password"));
        assert!(!COMMON.iter().any(|p| p.starts_with('#')));
    }

    #[test]
    fn what_a_new_password_must_not_be() {
        assert_eq!(check_new("short-one"), Err(Weak::TooShort));
        assert_eq!(check_new(&"x".repeat(MAX_LEN + 1)), Err(Weak::TooLong));
        assert_eq!(check_new("unbelievable"), Err(Weak::Common));
        assert_eq!(
            check_new("UnBelievable"),
            Err(Weak::Common),
            "case does not hide it"
        );
        assert_eq!(check_new("plenty of words that are fine"), Ok(()));
        assert_eq!(
            check_new("パスワードは十二文字以上必要です"),
            Ok(()),
            "counts characters, not bytes"
        );
    }

    #[test]
    fn the_account_and_company_name_are_allowed() {
        assert_eq!(check_new("GoliaXiaxi@123456789"), Ok(()));
        assert_eq!(check_new("my-LiHao-pass-2026"), Ok(()));
        assert_eq!(check_new("123456789@Golia"), Ok(()));
        assert_eq!(check_new("GOLIA2026"), Err(Weak::TooShort));
    }
}
