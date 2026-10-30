//! The one password check every way of logging in uses.

use axum::http::StatusCode;

use crate::WebState;

use mailrs_core_api::method::admin::AccountWithHashWire;

/// The account, when `password` is `address`'s password. An unknown
/// account, one with no password set, and one whose stored value is not an
/// argon2 hash are all `Ok(None)`, the same answer as a wrong password; a
/// store that cannot be read is an error.
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
            return Err(
                StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            );
        }
    };
    let hash = acct.password_hash.as_deref().unwrap_or_default();
    match mailrs_auth_guard::password::matches(password, hash) {
        true => Ok(Some(acct)),
        false => Ok(None),
    }
}

/// A new password refused, as the answer to give: `400` with the reason.
pub(crate) fn refused(weak: mailrs_auth_guard::password::Weak) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        StatusCode::BAD_REQUEST,
        axum::Json(serde_json::json!({ "error": weak.to_string() })),
    )
        .into_response()
}
