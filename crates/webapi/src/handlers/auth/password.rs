//! The one password check every way of logging in uses.

use argon2::Argon2;
use axum::http::StatusCode;
use password_hash::{PasswordHash, PasswordVerifier};

use crate::WebState;

use mailrs_core_api::method::admin::AccountWithHashWire;

/// The account, when `password` is `address`'s password. An unknown
/// account or one with no password set is `Ok(None)`, the same answer as a
/// wrong password; a store that cannot be read or a hash that does not
/// parse is an error.
pub(crate) async fn verify(
    state: &WebState,
    address: &str,
    password: &str,
) -> Result<Option<AccountWithHashWire>, StatusCode> {
    let acct = match state.core.get_account_with_hash(address).await {
        Ok(a) => a,
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => return Ok(None),
        Err(e) => {
            tracing::warn!(error = %e, "password check: get_account_with_hash failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let hash = match acct.password_hash.as_deref() {
        Some(h) if !h.is_empty() => h,
        _ => return Ok(None),
    };
    let parsed = PasswordHash::new(hash).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_err()
    {
        return Ok(None);
    }
    Ok(Some(acct))
}

/// Shortest new password a person may set.
pub(crate) const MIN_NEW_LEN: usize = 8;
/// Longest, so a request cannot make the hash work arbitrarily long.
pub(crate) const MAX_NEW_LEN: usize = 1024;

/// Whether `password` may be set as a new password.
pub(crate) fn acceptable_new(password: &str) -> bool {
    (MIN_NEW_LEN..=MAX_NEW_LEN).contains(&password.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_passwords_have_a_length_range() {
        assert!(!acceptable_new(""));
        assert!(!acceptable_new("1234567"));
        assert!(acceptable_new("12345678"));
        assert!(acceptable_new("パスワード漢字八字"));
        assert!(!acceptable_new(&"x".repeat(MAX_NEW_LEN + 1)));
    }
}
