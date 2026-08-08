//! `POST /api/mail/unsubscribe` — the one-click leave (RFC 8058).
//!
//! The POST is made **here**, not by the client, and that is the point
//! of the endpoint existing at all. The URL in `List-Unsubscribe`
//! identifies one subscriber; fetching it from a phone tells the sender
//! the reader opened the mail, from which address, on which network, at
//! what moment. The same reason the message body blocks remote loads.
//!
//! Only the URL the message itself carried is posted to. A client that
//! could name any URL would have a request forwarder here, pointed at
//! whatever it liked from inside the network the server sits in.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

#[derive(Debug, serde::Deserialize)]
pub struct UnsubscribeRequest {
    /// The thread the message is in.
    pub thread_id: String,
    /// Which message in it — `uid`, the same identity the timeline uses.
    pub uid: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct UnsubscribeResult {
    pub ok: bool,
    /// What the sender's endpoint answered, for the log. Absent when
    /// the request never got that far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Short, and not retried. An unsubscribe that fails is a link the
/// reader can still open; an unsubscribe that hangs is a spinner with
/// nothing behind it.
fn http_client() -> Result<reqwest::Client, StatusCode> {
    reqwest::Client::builder()
        .user_agent("mailrs-unsubscribe/1.0")
        .timeout(Duration::from_secs(10))
        // One hop. RFC 8058 endpoints redirect to a confirmation page
        // often enough that refusing entirely would fail live senders,
        // but a chain is a chain.
        .redirect(reqwest::redirect::Policy::limited(1))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// `POST /api/mail/unsubscribe`
///
/// Reads the named message, takes the one-click URL out of its own
/// `List-Unsubscribe` header, and posts RFC 8058's fixed body to it.
/// 404 when that message advertises no one-click target — the client
/// should be offering the link instead, and asking here for one that
/// does not exist is a bug rather than a user error.
pub(crate) async fn unsubscribe(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<UnsubscribeRequest>,
) -> Result<Json<UnsubscribeResult>, StatusCode> {
    let url = one_click_url_for(&state, &user, &req).await?;

    let response = http_client()?
        .post(&url)
        // RFC 8058 §3.1: exactly this body, and this content type.
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("List-Unsubscribe=One-Click")
        .send()
        .await;

    match response {
        Ok(r) => {
            let status = r.status();
            Ok(Json(UnsubscribeResult {
                ok: status.is_success(),
                status: Some(status.as_u16()),
                message: None,
            }))
        }
        // A refusal by the sender's endpoint is reported, not turned
        // into a 5xx: the request was fine, the other end was not, and
        // the client needs to say which.
        Err(e) => Ok(Json(UnsubscribeResult {
            ok: false,
            status: None,
            message: Some(e.to_string()),
        })),
    }
}

/// The message's own one-click URL, or a 404.
async fn one_click_url_for(
    state: &Arc<WebState>,
    user: &str,
    req: &UnsubscribeRequest,
) -> Result<String, StatusCode> {
    let messages = crate::handlers::conversations::thread_messages_for(state, user, &req.thread_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let message = messages
        .into_iter()
        .find(|m| m.uid == req.uid)
        .ok_or(StatusCode::NOT_FOUND)?;
    let unsub = message.unsubscribe.ok_or(StatusCode::NOT_FOUND)?;
    if !unsub.one_click {
        return Err(StatusCode::NOT_FOUND);
    }
    // The same https-only rule the stone applies, restated where the
    // request is actually made: whatever route reaches this line, the
    // token does not go out in the clear.
    unsub
        .http
        .into_iter()
        .find(|u| u.len() >= 8 && u[..8].eq_ignore_ascii_case("https://"))
        .ok_or(StatusCode::NOT_FOUND)
}
