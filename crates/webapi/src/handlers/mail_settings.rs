//! Forwarding to an address elsewhere and the vacation reply, set by the
//! signed-in user. Both act on mail as it arrives, in either core's drain.
//!
//! Pointing mail at a new address needs the password again (a stolen
//! session must not be able to send itself someone's mail) and the code
//! sent to that address (nobody's mail goes to a stranger who did not ask).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::forwarding::{self, Forwarding, Verified};
use mailrs_core_sidestate::families::vacation::{self, Vacation};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::rules::refused;

/// GET /api/settings/vacation — off with empty fields when never set.
pub async fn get_vacation(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vacation>, StatusCode> {
    let v = with_kevy(move |c| vacation::get(c, &user))?;
    Ok(Json(v.unwrap_or(Vacation {
        enabled: false,
        subject: String::new(),
        body: String::new(),
        start: None,
        end: None,
        days: 7,
        version: 0,
    })))
}

/// PUT /api/settings/vacation
pub async fn put_vacation(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(v): Json<Vacation>,
) -> Response {
    match with_kevy(move |c| vacation::set(c, &user, v)) {
        Ok(Ok(saved)) => Json(saved).into_response(),
        Ok(Err(why)) => refused(why.code(), StatusCode::BAD_REQUEST),
        Err(status) => status.into_response(),
    }
}

/// What the settings page shows: never the code.
#[derive(serde::Serialize)]
pub struct ForwardingView {
    pub address: String,
    pub keep_copy: bool,
    pub pending_address: String,
}

fn view(f: Forwarding) -> ForwardingView {
    ForwardingView {
        address: f.address,
        keep_copy: f.keep_copy,
        pending_address: f.pending_address,
    }
}

/// GET /api/settings/forwarding
pub async fn get_forwarding(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<ForwardingView>, StatusCode> {
    Ok(Json(view(with_kevy(move |c| forwarding::get(c, &user))?)))
}

#[derive(Debug, serde::Deserialize)]
pub struct ForwardRequest {
    pub address: String,
    #[serde(default = "yes")]
    pub keep_copy: bool,
}

fn yes() -> bool {
    true
}

/// POST /api/settings/forwarding `{address, keep_copy}` — send a code to
/// `address`; forwarding to it starts once the code comes back.
pub async fn request_forwarding(
    _: crate::session::confirm::Confirmed,
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<ForwardRequest>,
) -> Response {
    let address = req.address.trim().to_lowercase();
    if !looks_like_an_address(&address) {
        return refused("invalid_address", StatusCode::BAD_REQUEST);
    }
    if address == user.to_lowercase() {
        return refused("own_address", StatusCode::BAD_REQUEST);
    }
    let now = chrono::Utc::now().timestamp();
    let (who, to) = (user.clone(), address.clone());
    let code = match with_kevy(move |c| forwarding::request(c, &who, &to, req.keep_copy, now)) {
        Ok(code) => code,
        Err(status) => return status.into_response(),
    };
    let text = format!(
        "{user} asked for their mail to be forwarded to this address.\n\n\
         To allow it, enter this code in their mail settings: {code}\n\n\
         The code works for 24 hours. If you did not expect this, ignore this \
         message and nothing will be forwarded.\n"
    );
    if let Err(status) =
        super::prefs::send_system_mail(&address, "Confirm mail forwarding", &text, "")
    {
        return status.into_response();
    }
    super::audit::record(&user, "forwarding.request", &address, "");
    StatusCode::ACCEPTED.into_response()
}

#[derive(Debug, serde::Deserialize)]
pub struct VerifyRequest {
    pub code: String,
}

/// POST /api/settings/forwarding/verify `{code}`
pub async fn verify_forwarding(
    State(state): State<Arc<WebState>>,
    crate::login_guard::ClientIp(source): crate::login_guard::ClientIp,
    headers: axum::http::HeaderMap,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<VerifyRequest>,
) -> Response {
    let now = chrono::Utc::now().timestamp();
    let who = user.clone();
    match with_kevy(move |c| forwarding::verify(c, &who, &req.code, now)) {
        Ok(Verified::On(address)) => {
            super::audit::record(&user, "forwarding.on", &address, "");
            super::owner_notice::notify_owner(
                Some(&state),
                &user,
                crate::security_mail::Notice::ForwardingOn(address.clone()),
                super::owner_notice::origin_of(&headers, &source.to_string()),
                &[],
            )
            .await;
            Json(serde_json::json!({ "address": address })).into_response()
        }
        Ok(Verified::Wrong(left)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "wrong_code", "tries_left": left })),
        )
            .into_response(),
        Ok(Verified::Nothing) => refused("no_pending_code", StatusCode::BAD_REQUEST),
        Err(status) => status.into_response(),
    }
}

/// DELETE /api/settings/forwarding — stop forwarding.
pub async fn stop_forwarding(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> StatusCode {
    let who = user.clone();
    match with_kevy(move |c| forwarding::off(c, &who)) {
        Ok(()) => {
            super::audit::record(&user, "forwarding.off", "", "");
            StatusCode::NO_CONTENT
        }
        Err(status) => status,
    }
}

fn looks_like_an_address(a: &str) -> bool {
    let Some((local, domain)) = a.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !a.contains(char::is_whitespace)
        && !a.contains(['<', '>', ',', ';'])
        && a.len() <= 254
}
