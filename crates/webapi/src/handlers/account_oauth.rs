//! `/api/accounts/external/oauth/*` — connecting a Gmail or an Outlook.
//!
//! The same dance `external_login` does, aimed at a different question.
//! That flow asks *who is this*; this one asks for **a year of access
//! to a mailbox**, which is why it needs two things a login never did:
//! the mailbox scopes, and a refresh token to renew with.
//!
//! Both providers refuse passwords for mail clients outright, so
//! without this there is no way to connect a Gmail at all — the set-up
//! form says as much and offers nothing.
//!
//! **The application must be registered.** A client id and secret come
//! from Google and from Microsoft; unset, every route here answers 503
//! with that as the reason rather than starting a flow that cannot
//! finish.

use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use mailrs_oauth_client::{IdentitySource, Pkce, Provider, mailbox_scopes};
use serde::Deserialize;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// How long a half-finished connect flow is kept.
///
/// Long enough to sign in and approve, short enough that a state token
/// captured from a shared machine is stale before it is useful.
const FLOW_TTL: std::time::Duration = std::time::Duration::from_secs(600);

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

/// Why a provider cannot be connected, for a screen to show.
///
/// Public so a test can assert on the words rather than on the source
/// file — a string continuation splits them across lines, and a test
/// reading the file was arguing with the formatter.
pub fn why_not_registered(key: &str) -> String {
    format!(
        "this server has not registered an application with {key}, so its \
         mailboxes cannot be connected — {key} does not accept a password \
         for mail clients"
    )
}

fn not_registered(key: &str) -> (StatusCode, String) {
    (StatusCode::SERVICE_UNAVAILABLE, why_not_registered(key))
}

/// `GET /api/accounts/external/oauth/{provider}` — begin.
pub async fn start(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(key): Path<String>,
) -> axum::response::Response {
    let Some(provider) = mail_provider(&key) else {
        return not_registered(&key).into_response();
    };
    let state_tok = crate::handlers::external_login::random_hex(16);
    let pkce = Pkce::generate();

    // The verifier stays here; the browser carries only the state that
    // names it. An authorization code intercepted in the redirect is
    // useless without this row.
    let stored = serde_json::json!({
        "provider": key,
        "verifier": pkce.verifier,
        "user": user,
    });
    let state_key = format!("mailoauth:flow:{state_tok}");
    let payload = stored.to_string();
    if with_kevy(move |c| {
        c.set_with_ttl(state_key.as_bytes(), payload.as_bytes(), FLOW_TTL)
            .map_err(std::io::Error::from)
    })
    .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Redirect::to(&mailrs_oauth_client::authorize_url(
        &provider, &state_tok, &pkce,
    ))
    .into_response()
}

/// What the provider sends back.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    /// The authorization code, on success.
    #[serde(default)]
    pub code: String,
    /// The state token, naming the flow this belongs to.
    #[serde(default)]
    pub state: String,
    /// Set instead of `code` when the person declined.
    #[serde(default)]
    pub error: String,
}

/// `GET /api/accounts/external/oauth/callback` — finish.
///
/// Declining is not a failure: somebody looked at the consent screen
/// and said no, and the honest answer is the set-up page again rather
/// than an error.
pub async fn callback(Query(q): Query<CallbackQuery>) -> axum::response::Response {
    if !q.error.is_empty() {
        return Redirect::to("/settings?tab=mail-accounts&connect=declined").into_response();
    }
    if q.code.is_empty() || q.state.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing code or state").into_response();
    }
    // The flow row is single-use: read it, delete it, and a replayed
    // callback finds nothing.
    let key = format!("mailoauth:flow:{}", q.state);
    let read_key = key.clone();
    let stored = with_kevy(move |c| c.get(read_key.as_bytes()).map_err(std::io::Error::from))
        .ok()
        .flatten();
    let _ = with_kevy(move |c| c.del(&[key.as_bytes()]).map_err(std::io::Error::from));
    let Some(raw) = stored else {
        return (StatusCode::BAD_REQUEST, "this sign-in has expired").into_response();
    };
    let Ok(flow) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return (StatusCode::BAD_REQUEST, "unreadable flow").into_response();
    };
    let (Some(provider_key), Some(verifier), Some(user)) = (
        flow["provider"].as_str(),
        flow["verifier"].as_str(),
        flow["user"].as_str(),
    ) else {
        return (StatusCode::BAD_REQUEST, "incomplete flow").into_response();
    };
    let Some(provider) = mail_provider(provider_key) else {
        return not_registered(provider_key).into_response();
    };

    let secret = std::env::var(match provider_key {
        "google" => "MAILRS_GOOGLE_MAIL_CLIENT_SECRET",
        _ => "MAILRS_MICROSOFT_MAIL_CLIENT_SECRET",
    })
    .unwrap_or_default();
    let body = mailrs_oauth_client::token_request_body(
        &provider,
        &q.code,
        &Pkce::from_verifier(verifier),
        &secret,
    );
    let Ok(resp) = reqwest::Client::new()
        .post(&provider.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
    else {
        return (StatusCode::BAD_GATEWAY, "could not reach the provider").into_response();
    };
    let Ok(bytes) = resp.bytes().await else {
        return (StatusCode::BAD_GATEWAY, "no answer from the provider").into_response();
    };
    let Ok(token) = mailrs_oauth_client::parse_token_response(&provider, &bytes) else {
        return (StatusCode::BAD_GATEWAY, "unreadable token answer").into_response();
    };
    // Without a refresh token the account works for an hour and then
    // asks to sign in again, with nothing in the flow saying why. Say
    // it now instead.
    let Some(refresh) = token.refresh_token.clone() else {
        return (
            StatusCode::BAD_GATEWAY,
            "the provider returned no refresh token, so this account could only \
             be read for an hour — the application's scopes are missing \
             offline access",
        )
            .into_response();
    };

    // The address is the provider's, read from the id token it just
    // signed. Asking the person to type it would let a typo produce an
    // account that authenticates as one mailbox and is labelled as
    // another.
    let email = token
        .id_token
        .as_deref()
        .and_then(|t| mailrs_oauth_client::jwt_claims_unverified(t).ok())
        .and_then(|c| c.get("email").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_default();
    if email.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            "the provider did not say which mailbox",
        )
            .into_response();
    }

    match super::external_accounts::connect_oauth_account(
        user,
        provider_key,
        &email,
        &token.access_token,
        &refresh,
        token.expires_in.unwrap_or(3600),
    ) {
        Ok(()) => Redirect::to("/settings?tab=mail-accounts&connect=ok").into_response(),
        Err((code, why)) => (code, why).into_response(),
    }
}
