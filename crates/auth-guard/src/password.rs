//! Passwords: the one check a sign-in makes ([`matches()`]), and what a new
//! password must not be ([`check_new`]). The second is checked whenever a
//! password is set — by its owner, by a reset, by an administrator — and
//! never at sign-in: an existing password keeps working until its owner
//! changes it.

use std::collections::HashSet;
use std::sync::LazyLock;

use argon2::Argon2;
use password_hash::{PasswordHash, PasswordVerifier};

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

/// What a sign-in to an account whose password an administrator cleared is
/// told, in every protocol: that the password was reset, and whom to ask for
/// a code to set a new one, from `MAILRS_RESET_CONTACT_NAME` and
/// `MAILRS_RESET_CONTACT`. ASCII only, as protocol responses must be: a
/// name written in another script is left out and the contact kept.
pub fn cleared_notice() -> String {
    let read = |name| std::env::var(name).unwrap_or_default();
    notice(
        &read("MAILRS_RESET_CONTACT_NAME"),
        &read("MAILRS_RESET_CONTACT"),
    )
}

fn notice(name: &str, contact: &str) -> String {
    let ascii = |s: &str| {
        let s = s.trim();
        match s.is_ascii() && !s.chars().any(|c| c.is_ascii_control() || c == '"') {
            true => s.to_string(),
            false => String::new(),
        }
    };
    let who = match (ascii(name), ascii(contact)) {
        (n, c) if !n.is_empty() && !c.is_empty() => format!("{n} <{c}>"),
        (n, c) if n.is_empty() && c.is_empty() => "an administrator".to_string(),
        (n, c) => format!("{n}{c}"),
    };
    format!(
        "Your password was reset by an administrator. Ask {who} for a reset code to set a new one"
    )
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
    /// Contains the account's name or its domain's.
    ContainsName,
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
            Weak::ContainsName => write!(
                f,
                "Leave the account name and the domain out of the password"
            ),
        }
    }
}

/// Whether `password` may become `account`'s password.
pub fn check_new(password: &str, account: &str) -> Result<(), Weak> {
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
    if names(account).iter().any(|n| lower.contains(n.as_str())) {
        return Err(Weak::ContainsName);
    }
    Ok(())
}

/// The parts of `account` a password should not contain: the local part
/// and each domain label but the last, three characters or longer.
fn names(account: &str) -> Vec<String> {
    let account = account.trim().to_lowercase();
    let (local, domain) = account.split_once('@').unwrap_or((account.as_str(), ""));
    let labels: Vec<&str> = domain.split('.').collect();
    let keep = labels.len().saturating_sub(1);
    std::iter::once(local)
        .chain(labels.into_iter().take(keep))
        .filter(|n| n.chars().count() >= 3)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashed(password: &str) -> String {
        use password_hash::{PasswordHasher, SaltString};
        let salt = SaltString::from_b64("c29tZXNhbHRzb21lc2FsdA").expect("salt");
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
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
        assert_eq!(
            notice("Li Hao", "lihao@golia.jp"),
            "Your password was reset by an administrator. Ask Li Hao <lihao@golia.jp> for a reset code to set a new one"
        );
        // a name in another script cannot go into a protocol line; the address can
        assert_eq!(
            notice("李浩", "lihao@golia.jp"),
            "Your password was reset by an administrator. Ask lihao@golia.jp for a reset code to set a new one"
        );
        assert_eq!(
            notice("", ""),
            "Your password was reset by an administrator. Ask an administrator for a reset code to set a new one"
        );
        assert!(notice("a\"b", "x\r\ny").contains("Ask an administrator"));
    }

    #[test]
    fn the_list_is_loaded() {
        assert!(COMMON.len() >= 10_000, "{}", COMMON.len());
        assert!(COMMON.contains("password"));
        assert!(!COMMON.iter().any(|p| p.starts_with('#')));
    }

    #[test]
    fn what_a_new_password_must_not_be() {
        let a = "lihao@golia.jp";
        assert_eq!(check_new("short-one", a), Err(Weak::TooShort));
        assert_eq!(check_new(&"x".repeat(MAX_LEN + 1), a), Err(Weak::TooLong));
        assert_eq!(check_new("unbelievable", a), Err(Weak::Common));
        assert_eq!(
            check_new("UnBelievable", a),
            Err(Weak::Common),
            "case does not hide it"
        );
        assert_eq!(check_new("my-LiHao-pass-2026", a), Err(Weak::ContainsName));
        assert_eq!(check_new("ilovegolia-forever", a), Err(Weak::ContainsName));
        assert_eq!(check_new("plenty of words that are fine", a), Ok(()));
        assert_eq!(
            check_new("パスワードは十二文字以上必要です", a),
            Ok(()),
            "counts characters, not bytes"
        );
        // a two-letter TLD or a short name is not a name worth refusing
        assert_eq!(check_new("jp-and-a-long-tail-here", "ab@cd.jp"), Ok(()));
    }
}
