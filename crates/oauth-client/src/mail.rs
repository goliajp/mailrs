//! The mailbox half of OAuth: the scopes a mail client asks for,
//! when a token is worth renewing, and the two providers that serve
//! mail this way. Split from `lib.rs` at the file-size gate.

use crate::{IdentitySource, Provider, encode};

/// How long before expiry a token is renewed.
///
/// A minute is enough to finish a sync that has already started and
/// short enough that a clock a little out of step does not renew on
/// every tick. The number matters less than which side of expiry it
/// is on: **before**, always.
pub const RENEW_WITHIN_SECS: i64 = 300;

/// Whether an access token should be renewed now.
///
/// An unknown expiry (`0`) is due rather than assumed fresh: it is
/// either from before this existed or was written by something that
/// did not say, and asking once is cheaper than a mailbox that quietly
/// stops.
pub fn needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at == 0 || now + RENEW_WITHIN_SECS >= expires_at
}

/// The scopes a provider needs for its mail, or empty for one whose
/// mailbox this cannot read.
///
/// `offline_access` is not decoration: without it the provider returns
/// **no refresh token at all**, and the account works for one hour and
/// then asks to sign in again with nothing in the flow saying why.
/// Google spells the same thing `access_type=offline`, which
/// [`authorize_url`] adds for that provider.
pub fn mailbox_scopes(provider_key: &str) -> String {
    match provider_key {
        "google" => "https://mail.google.com/".into(),
        "microsoft" => concat!(
            "offline_access ",
            "https://outlook.office.com/IMAP.AccessAsUser.All ",
            "https://outlook.office.com/SMTP.Send"
        )
        .into(),
        _ => String::new(),
    }
}

/// The body that renews an access token.
///
/// The same shape as [`token_request_body`] with `grant_type` swapped:
/// providers differ on almost everything else and agree on this.
pub fn refresh_request_body(p: &Provider, refresh_token: &str, client_secret: &str) -> String {
    let mut body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        encode(refresh_token),
        encode(&p.client_id)
    );
    if !client_secret.is_empty() {
        body.push_str(&format!("&client_secret={}", encode(client_secret)));
    }
    body
}

/// The providers this can connect, and where their endpoints are.
///
/// `None` when the deployment has not registered an application with
/// that provider — which is the common case and is said plainly rather
/// than guessed around.
pub fn mail_provider(key: &str) -> Option<Provider> {
    let (env_id, env_secret) = match key {
        "google" => (
            "MAILRS_GOOGLE_MAIL_CLIENT_ID",
            "MAILRS_GOOGLE_MAIL_CLIENT_SECRET",
        ),
        "microsoft" => (
            "MAILRS_MICROSOFT_MAIL_CLIENT_ID",
            "MAILRS_MICROSOFT_MAIL_CLIENT_SECRET",
        ),
        _ => return None,
    };
    let client_id = std::env::var(env_id)
        .ok()
        .filter(|v| !v.trim().is_empty())?;
    let _ = env_secret;
    let redirect_uri = std::env::var("MAILRS_MAIL_OAUTH_REDIRECT")
        .ok()
        .filter(|v| !v.trim().is_empty())?;
    let (issuer, authorize_url, token_url) = match key {
        "google" => (
            "https://accounts.google.com",
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
        ),
        _ => (
            "https://login.microsoftonline.com/common/v2.0",
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        ),
    };
    Some(Provider {
        key: key.to_string(),
        issuer: issuer.to_string(),
        authorize_url: authorize_url.to_string(),
        token_url: token_url.to_string(),
        userinfo_url: None,
        // The mailbox scopes, which are the point. `offline_access` is
        // in there for Microsoft and Google's equivalent is a query
        // parameter — without it neither returns a refresh token, and
        // the account works for one hour.
        scopes: mailbox_scopes(key)
            .split(' ')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        client_id,
        redirect_uri,
        source: IdentitySource::IdToken,
        require_verified_email: false,
    })
}
