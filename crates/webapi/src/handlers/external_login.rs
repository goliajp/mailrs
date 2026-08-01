//! Signing in with an identity from somewhere else.
//!
//! Third-party login is an authentication method, not a source of accounts.
//! An account exists in mailrs or it does not; this is another way of proving
//! you own one, alongside the password. Nothing here creates an account and
//! nothing here looks one up by email.
//!
//! ```text
//!   GET /api/auth/external/{provider}          -> redirect to the provider
//!   GET /api/auth/oidc/callback?code&state     -> session, or a link prompt
//! ```
//!
//! Linked identity → straight in. Unlinked → the identity is parked
//! server-side under a single-use handle and the browser is sent to the login
//! page; the password login that follows claims the handle and writes the
//! link. See `.claude/rfcs/20260801-external-login.md`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header::SET_COOKIE};
use axum::response::{IntoResponse, Redirect};
use mailrs_oauth_client::{ExternalIdentity, IdentitySource, Pkce, Provider};
use serde::Deserialize;

use crate::WebState;
use crate::handlers::kevy_util::with_kevy;

/// How long a browser has to come back from the provider.
const FLOW_TTL: std::time::Duration = std::time::Duration::from_secs(600);

fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn redirect_uri() -> String {
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

fn provider_by_key(key: &str) -> Option<(Provider, String)> {
    configured_providers()
        .into_iter()
        .find(|(p, _)| p.key == key)
}

/// `GET /api/auth/external/{provider}` — start the flow.
pub async fn start(Path(key): Path<String>) -> impl IntoResponse {
    let Some((provider, _)) = provider_by_key(&key) else {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "provider not configured" })),
        )
            .into_response();
    };
    let state_tok = random_hex(16);
    let pkce = Pkce::generate();

    // The verifier stays here. What goes to the browser is the state, which
    // names it — an authorization code intercepted in the redirect is
    // useless without the verifier this row holds.
    let stored = serde_json::json!({ "provider": key, "verifier": pkce.verifier });
    let state_key = format!("oidc:flow:{state_tok}");
    let payload = stored.to_string();
    if with_kevy(move |c| {
        c.set_with_ttl(state_key.as_bytes(), payload.as_bytes(), FLOW_TTL)
            .map_err(std::io::Error::other)
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

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

fn login_redirect(reason: &str) -> axum::response::Response {
    Redirect::to(&format!("/login?external={reason}")).into_response()
}

/// `GET /api/auth/oidc/callback` — finish it.
pub async fn callback(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> axum::response::Response {
    if q.error.is_some() {
        return login_redirect("denied");
    }
    let (Some(code), Some(state_tok)) = (q.code, q.state) else {
        return login_redirect("incomplete");
    };

    // Single-use: a replayed state finds nothing, so a captured callback URL
    // cannot be walked a second time.
    let flow_key = format!("oidc:flow:{state_tok}");
    let flow = with_kevy(move |c| {
        let got = c.get(flow_key.as_bytes()).map_err(std::io::Error::other)?;
        if got.is_some() {
            c.del(&[flow_key.as_bytes()])
                .map_err(std::io::Error::other)?;
        }
        Ok(got)
    })
    .ok()
    .flatten();
    let Some(flow) = flow.and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
        return login_redirect("expired");
    };
    let key = flow["provider"].as_str().unwrap_or_default().to_string();
    let verifier = flow["verifier"].as_str().unwrap_or_default().to_string();
    let Some((provider, secret)) = provider_by_key(&key) else {
        return login_redirect("unavailable");
    };

    let identity = match exchange(&provider, &secret, &code, &verifier).await {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(provider = %key, err = %e, "external login: no identity");
            return login_redirect("failed");
        }
    };

    let (iss, sub) = (identity.issuer.clone(), identity.subject.clone());
    let linked = with_kevy(move |c| {
        mailrs_core_sidestate::families::identity_link::account_for(c, &iss, &sub)
    })
    .ok()
    .flatten();

    match linked {
        // Linked: this identity is the credential.
        Some(address) => match state.core.get_account_with_hash(&address).await {
            Ok(acct) => {
                crate::handlers::audit::record(&address, "auth.login.external", &key, "");
                crate::handlers::auth::issue_session(&state, &acct).await
            }
            // Linked to an account that no longer exists. Refusing is the
            // only safe answer — the alternative is inventing one.
            Err(_) => login_redirect("no-account"),
        },
        // Not linked, but already signed in — this is the "add a sign-in
        // method" button in settings. The session is the proof that would
        // otherwise be asked for, so there is nothing to park: link it and
        // say so. The callback is a top-level GET, so the SameSite=Lax
        // session cookie arrives with it.
        None if crate::session::resolve_user_from_headers(&headers)
            .await
            .is_some() =>
        {
            let Some(address) = crate::session::resolve_user_from_headers(&headers).await else {
                return login_redirect("failed");
            };
            let (iss, sub) = (identity.issuer.clone(), identity.subject.clone());
            let addr = address.clone();
            let outcome = with_kevy(move |c| {
                mailrs_core_sidestate::families::identity_link::link(c, &iss, &sub, &addr)
            });
            use mailrs_core_sidestate::families::identity_link::LinkOutcome;
            match outcome {
                Ok(LinkOutcome::Linked) | Ok(LinkOutcome::AlreadyLinked) => {
                    crate::handlers::audit::record(&address, "auth.identity.link", &key, "");
                    Redirect::to("/settings?linked=1").into_response()
                }
                // Held by somebody else. Never moved silently — that is one
                // person taking over another's sign-in.
                Ok(LinkOutcome::TakenByAnotherAccount) => {
                    Redirect::to("/settings?linked=taken").into_response()
                }
                Err(_) => Redirect::to("/settings?linked=failed").into_response(),
            }
        }
        // Not linked and not signed in: park it and ask for a password once.
        // Parking rather than passing it back means the browser cannot edit
        // whose identity this is.
        None => {
            let handle = random_hex(16);
            let json = serde_json::to_string(&identity).unwrap_or_default();
            let h = handle.clone();
            if with_kevy(move |c| {
                mailrs_core_sidestate::families::identity_link::park_pending(c, &h, &json)
            })
            .is_err()
            {
                return login_redirect("failed");
            }
            // A cookie, not a query parameter: a handle in the URL survives
            // in history and in the referrer sent to whatever loads next.
            let cookie = format!(
                "mailrs_pending_link={handle}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=600"
            );
            let mut resp = login_redirect("link");
            if let Ok(v) = axum::http::HeaderValue::from_str(&cookie) {
                resp.headers_mut().insert(SET_COOKIE, v);
            }
            resp
        }
    }
}

/// Exchange the code and read the identity out of whatever comes back.
async fn exchange(
    provider: &Provider,
    secret: &str,
    code: &str,
    verifier: &str,
) -> Result<ExternalIdentity, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let pkce = Pkce {
        verifier: verifier.to_string(),
        challenge: String::new(),
    };
    let body = mailrs_oauth_client::token_request_body(provider, code, &pkce, secret);
    let resp = client
        .post(&provider.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        // GitHub answers form-encoded unless asked for JSON.
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let tokens =
        mailrs_oauth_client::parse_token_response(provider, &bytes).map_err(|e| e.to_string())?;

    match provider.source {
        IdentitySource::IdToken => {
            let id_token = tokens.id_token.ok_or("no id_token")?;
            let claims =
                mailrs_oauth_client::jwt_claims_unverified(&id_token).map_err(|e| e.to_string())?;
            mailrs_oauth_client::identity_from_claims(provider, &claims).map_err(|e| e.to_string())
        }
        IdentitySource::UserinfoThenEmails => {
            let url = provider.userinfo_url.as_deref().ok_or("no userinfo url")?;
            let user: serde_json::Value = client
                .get(url)
                .bearer_auth(&tokens.access_token)
                .header("user-agent", "mailrs")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            // The second call is the point: `user.email` is the public
            // profile address, typed by the account holder and never
            // verified.
            let emails: serde_json::Value = client
                .get(format!("{url}/emails"))
                .bearer_auth(&tokens.access_token)
                .header("user-agent", "mailrs")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .unwrap_or(serde_json::Value::Array(vec![]));
            mailrs_oauth_client::identity_from_github(provider, &user, &emails)
                .map_err(|e| e.to_string())
        }
    }
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

/// `GET /api/auth/identities` — the sign-in methods on this account.
pub async fn list_identities(
    axum::extract::Extension(crate::handlers::conversations::AuthedUser(user)): axum::extract::Extension<
        crate::handlers::conversations::AuthedUser,
    >,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    let links =
        with_kevy(move |c| mailrs_core_sidestate::families::identity_link::links_for(c, &user))
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // The provider key is derived from the issuer so the UI can show a name
    // and an icon; the issuer is what is actually stored.
    let items: Vec<serde_json::Value> = links
        .into_iter()
        .map(|(issuer, subject)| {
            let key = configured_providers()
                .into_iter()
                .find(|(p, _)| p.issuer == issuer)
                .map(|(p, _)| p.key);
            serde_json::json!({ "issuer": issuer, "subject": subject, "provider": key })
        })
        .collect();
    Ok(axum::Json(serde_json::json!({ "items": items })))
}

/// What to unlink.
#[derive(Debug, Deserialize)]
pub struct UnlinkRequest {
    /// The issuer half of the link.
    pub issuer: String,
    /// The provider's identifier for the person.
    pub subject: String,
}

/// `POST /api/auth/identities:unlink` — remove one sign-in method.
///
/// The account is the session's, never the body's: naming somebody else's
/// identity must not remove it. `unlink` enforces that too — this is the
/// same rule stated at both ends on purpose, because it is the one that
/// keeps a link from being detached by whoever can guess it.
pub async fn unlink_identity(
    axum::extract::Extension(crate::handlers::conversations::AuthedUser(user)): axum::extract::Extension<
        crate::handlers::conversations::AuthedUser,
    >,
    axum::Json(req): axum::Json<UnlinkRequest>,
) -> Result<StatusCode, StatusCode> {
    let addr = user.clone();
    let removed = with_kevy(move |c| {
        mailrs_core_sidestate::families::identity_link::unlink(c, &req.issuer, &req.subject, &addr)
    })
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    match removed {
        true => {
            crate::handlers::audit::record(&user, "auth.identity.unlink", &user, "");
            Ok(StatusCode::NO_CONTENT)
        }
        // Not yours, or not there. One answer for both: telling them apart
        // would say whether an identity exists on another account.
        false => Err(StatusCode::NOT_FOUND),
    }
}
