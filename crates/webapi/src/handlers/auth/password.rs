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
