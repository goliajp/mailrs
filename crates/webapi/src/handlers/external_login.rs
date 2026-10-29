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
//! link.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header::SET_COOKIE};
use axum::response::{IntoResponse, Redirect};
use mailrs_oauth_client::{ExternalIdentity, IdentitySource, Pkce, Provider};
use serde::Deserialize;

use crate::WebState;
use crate::handlers::kevy_util::with_kevy;

/// How long a browser has to come back from the provider.
mod providers;
use providers::provider_by_key;
pub use providers::{configured_providers, list_providers};

const FLOW_TTL: std::time::Duration = std::time::Duration::from_secs(600);

pub(crate) fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
    // The nonce comes back inside the id_token, which is what ties the token
    // to this sign-in rather than to any other made with the same client.
    let nonce = random_hex(16);
    let stored = serde_json::json!({ "provider": key, "verifier": pkce.verifier, "nonce": nonce });
    let state_key = format!("oidc:flow:{state_tok}");
    let payload = stored.to_string();
    if with_kevy(move |c| {
        c.set_with_ttl(state_key.as_bytes(), payload.as_bytes(), FLOW_TTL)
            .map_err(std::io::Error::from)
    })
    .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let mut url = mailrs_oauth_client::authorize_url(&provider, &state_tok, &pkce);
    if matches!(provider.source, IdentitySource::IdToken) {
        url.push_str("&nonce=");
        url.push_str(&nonce);
    }
    let mut resp = Redirect::to(&url).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&format!(
        "{FLOW_COOKIE}={state_tok}; Path=/api/auth/oidc; HttpOnly; Secure; SameSite=Lax; Max-Age=600"
    )) {
        resp.headers_mut().insert(SET_COOKIE, v);
    }
    resp
}

/// Names the sign-in this browser started. The `state` alone is not
/// enough: anyone can start a sign-in, and a callback URL carrying their
/// own code and state, opened in somebody else's browser, would sign that
/// browser in as them.
const FLOW_COOKIE: &str = "mailrs_oidc_flow";

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

/// The callback is a top-level navigation, so the login response body
/// (which carries the token for the apps) would be shown as a page. Keep
/// only its session cookie and send the browser back to the login page,
/// which loads the profile through the cookie.
fn signed_in_redirect(issued: axum::response::Response) -> axum::response::Response {
    if !issued.status().is_success() {
        return login_redirect("failed");
    }
    let mut resp = login_redirect("ok");
    for cookie in issued.headers().get_all(SET_COOKIE) {
        resp.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    resp
}

/// `GET /api/auth/oidc/callback` — finish it.
pub async fn callback(
    state: State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> axum::response::Response {
    let started_here =
        q.state.is_some() && crate::handlers::auth::cookie_value(&headers, FLOW_COOKIE) == q.state;
    let mut resp = if started_here || q.error.is_some() {
        finish(state, headers, q).await
    } else {
        login_redirect("failed")
    };
    resp.headers_mut().append(
        SET_COOKIE,
        axum::http::HeaderValue::from_static(
            "mailrs_oidc_flow=; Path=/api/auth/oidc; HttpOnly; Secure; SameSite=Lax; Max-Age=0",
        ),
    );
    resp
}

async fn finish(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
    q: CallbackQuery,
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
        let got = c.get(flow_key.as_bytes())?;
        if got.is_some() {
            c.del(&[flow_key.as_bytes()])?;
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
    let nonce = flow["nonce"].as_str().unwrap_or_default().to_string();
    let Some((provider, secret)) = provider_by_key(&key) else {
        return login_redirect("unavailable");
    };

    let identity = match exchange(&provider, &secret, &code, &verifier, &nonce).await {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(provider = %key, err = %e, "external login: no identity");
            return login_redirect("failed");
        }
    };

    let signed_in = crate::session::resolve_session_user_from_headers(&headers).await;
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
                signed_in_redirect(crate::handlers::auth::issue_session(&state, &acct).await)
            }
            // Linked to an account that no longer exists. Refusing is the
            // only safe answer — the alternative is inventing one.
            Err(_) => login_redirect("no-account"),
        },
        // Not linked, but already signed in — this is the "add a sign-in
        // method" button in settings. A linked identity signs in without the
        // password, so a session alone is not enough: it must have confirmed
        // the password recently, or a stolen one could add a way back in. The
        // callback is a top-level GET, so the SameSite=Lax session cookie
        // arrives with it.
        None if signed_in.is_some() => {
            let Some((address, session)) = signed_in else {
                return login_redirect("failed");
            };
            if crate::session::confirm::require(Some(&session)).is_err() {
                return Redirect::to("/settings?linked=confirm").into_response();
            }
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
    nonce: &str,
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
            let claims = super::id_token::verify(provider, &id_token, nonce).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::LOCATION;

    #[test]
    fn a_finished_sign_in_keeps_the_cookie_and_drops_the_token_body() {
        let mut issued = axum::Json(serde_json::json!({ "token": "secret" })).into_response();
        issued.headers_mut().insert(
            SET_COOKIE,
            "mailrs_session=secret; HttpOnly".parse().unwrap(),
        );
        let resp = signed_in_redirect(issued);
        assert!(resp.status().is_redirection());
        assert_eq!(resp.headers()[LOCATION], "/login?external=ok");
        assert_eq!(
            resp.headers()[SET_COOKIE],
            "mailrs_session=secret; HttpOnly"
        );
    }

    #[test]
    fn a_failed_session_goes_back_as_a_failure_without_a_cookie() {
        let resp = signed_in_redirect(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        assert_eq!(resp.headers()[LOCATION], "/login?external=failed");
        assert!(resp.headers().get(SET_COOKIE).is_none());
    }
}
