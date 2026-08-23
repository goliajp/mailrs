//! Renewing an access token, once, for both callers.
//!
//! The sync worker and the sender each hold their own store, so each
//! loads and seals its own credential — but the exchange with the
//! provider is the same exchange, and a second copy of it is a second
//! place for the refresh-token rule and the `invalid_grant` reading to
//! drift. It lives here instead.

use crate::{Provider, parse_token_response, refresh_request_body};

/// What a renewal produced.
#[derive(Debug, Clone)]
pub struct Renewed {
    /// The new access token.
    pub access: String,
    /// The refresh token to keep.
    ///
    /// A renewal answer usually omits it, and that means keep the one
    /// already held — reading its absence as "no refresh token" signs
    /// a person out an hour later.
    pub refresh: String,
    /// When the new access token lapses, in seconds since the epoch.
    pub expires_at: i64,
}

/// Why a renewal did not produce a token.
#[derive(Debug, thiserror::Error)]
pub enum RenewError {
    /// The refresh token itself was refused: RFC 6749 §5.2 makes that a
    /// 400. This is the one renewal failure that means signing in
    /// again — every other one may just be a provider having a bad
    /// minute, and marking the account broken for that is something
    /// only a person can undo.
    #[error("invalid_grant renewing {0}")]
    Refused(String),
    /// The provider could not be reached, or did not answer.
    #[error("could not reach {provider} to renew: {why}")]
    Unreachable {
        /// Which provider did not answer.
        provider: String,
        /// What went wrong reaching it.
        why: String,
    },
    /// The provider answered with something this crate cannot read.
    #[error("unreadable renewal answer: {0}")]
    Unreadable(String),
}

/// Exchange a refresh token for a fresh access token.
///
/// `now` is passed in rather than read, so the expiry arithmetic is
/// testable and so a caller that already knows the time does not read
/// the clock twice.
pub async fn renew(
    provider: &Provider,
    refresh: &str,
    client_secret: &str,
    account: &str,
    now: i64,
) -> Result<Renewed, RenewError> {
    let body = refresh_request_body(provider, refresh, client_secret);
    let resp = reqwest::Client::new()
        .post(&provider.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| RenewError::Unreachable {
            provider: provider.key.clone(),
            why: e.to_string(),
        })?;
    let refused = resp.status() == reqwest::StatusCode::BAD_REQUEST;
    let bytes = resp.bytes().await.map_err(|e| RenewError::Unreachable {
        provider: provider.key.clone(),
        why: e.to_string(),
    })?;
    if refused {
        return Err(RenewError::Refused(account.to_string()));
    }
    let token = parse_token_response(provider, &bytes)
        .map_err(|e| RenewError::Unreadable(e.to_string()))?;
    Ok(Renewed {
        access: token.access_token,
        refresh: token.refresh_token.unwrap_or_else(|| refresh.to_string()),
        expires_at: now + token.expires_in.unwrap_or(3600),
    })
}
