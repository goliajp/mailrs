//! What a new password must not be. Checked whenever a password is set —
//! by its owner, by a reset, by an administrator — and never at sign-in:
//! an existing password keeps working until its owner changes it.

use std::collections::HashSet;
use std::sync::LazyLock;

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
