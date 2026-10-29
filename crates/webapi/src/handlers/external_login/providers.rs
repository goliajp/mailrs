//! Which third-party sign-in providers this deployment has credentials for.

use mailrs_oauth_client::{IdentitySource, Provider};

pub(super) fn redirect_uri() -> String {
    std::env::var("MAILRS_EXTERNAL_LOGIN_REDIRECT_URI")
        .unwrap_or_else(|_| "https://mail.golia.jp/api/auth/oidc/callback".into())
}

/// The providers this deployment has credentials for.
///
/// Endpoints are compiled in rather than configured: Google's and GitHub's
/// are the same everywhere, and portal's were measured from the running
/// service on 2026-08-01 (it has no discovery document at the standard path
/// — that returns its SPA's HTML). What is deployment-specific is the client
/// id and secret, and those come from the environment like every other
/// secret here.
///
/// A provider with no client id configured is simply absent, so the login
/// page offers what actually works.
pub fn configured_providers() -> Vec<(Provider, String)> {
    let mut out = Vec::new();
    for (key, issuer, authorize, token, userinfo, scopes, source, id_env, secret_env) in [
        (
            "portal",
            "https://portal.golia.jp",
            "https://portal.golia.jp/oauth/authorize",
            "https://portal.golia.jp/oauth/token",
            Some("https://portal.golia.jp/oauth/userinfo"),
            "openid email profile",
            IdentitySource::IdToken,
            "MAILRS_OIDC_PORTAL_CLIENT_ID",
            "MAILRS_OIDC_PORTAL_CLIENT_SECRET",
        ),
        (
            "google",
            "https://accounts.google.com",
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
            None,
            "openid email profile",
            IdentitySource::IdToken,
            "MAILRS_OIDC_GOOGLE_CLIENT_ID",
            "MAILRS_OIDC_GOOGLE_CLIENT_SECRET",
        ),
        (
            "github",
            "https://github.com",
            "https://github.com/login/oauth/authorize",
            "https://github.com/login/oauth/access_token",
            Some("https://api.github.com/user"),
            "read:user user:email",
            IdentitySource::UserinfoThenEmails,
            "MAILRS_OIDC_GITHUB_CLIENT_ID",
            "MAILRS_OIDC_GITHUB_CLIENT_SECRET",
        ),
        (
            "microsoft",
            "https://login.microsoftonline.com/common/v2.0",
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            None,
            "openid email profile",
            IdentitySource::IdToken,
            "MAILRS_OIDC_MICROSOFT_CLIENT_ID",
            "MAILRS_OIDC_MICROSOFT_CLIENT_SECRET",
        ),
        (
            "apple",
            "https://appleid.apple.com",
            "https://appleid.apple.com/auth/authorize",
            "https://appleid.apple.com/auth/token",
            None,
            "openid email name",
            IdentitySource::IdToken,
            "MAILRS_OIDC_APPLE_CLIENT_ID",
            "MAILRS_OIDC_APPLE_CLIENT_SECRET",
        ),
    ] {
        let Ok(client_id) = std::env::var(id_env) else {
            continue;
        };
        if client_id.is_empty() {
            continue;
        }
        let secret = std::env::var(secret_env).unwrap_or_default();
        // Microsoft's issuer is tenant-specific. `common/v2.0` is the
        // multi-tenant placeholder and will not equal the `iss` a real token
        // carries, so a deployment using it must pin its own tenant here —
        // the stone refuses a mismatch rather than accepting any tenant.
        let issuer = match key {
            "microsoft" => {
                std::env::var("MAILRS_OIDC_MICROSOFT_ISSUER").unwrap_or_else(|_| issuer.to_string())
            }
            _ => issuer.to_string(),
        };
        out.push((
            Provider {
                key: key.into(),
                issuer,
                authorize_url: authorize.into(),
                token_url: token.into(),
                userinfo_url: userinfo.map(str::to_string),
                scopes: scopes.split(' ').map(str::to_string).collect(),
                client_id,
                redirect_uri: redirect_uri(),
                source,
                require_verified_email: true,
            },
            secret,
        ));
    }
    out
}

pub(super) fn provider_by_key(key: &str) -> Option<(Provider, String)> {
    configured_providers()
        .into_iter()
        .find(|(p, _)| p.key == key)
}

/// `GET /api/auth/external-providers` — what the login page can offer.
///
/// Unauthenticated on purpose: it is read before anybody has signed in, and
/// it returns only which providers exist, never a client secret.
pub async fn list_providers() -> axum::Json<serde_json::Value> {
    let keys: Vec<String> = configured_providers()
        .into_iter()
        .map(|(p, _)| p.key)
        .collect();
    axum::Json(serde_json::json!({ "providers": keys }))
}
