//! Re-sending a message that already went out, and the id the copy gets.

//! The Send list — one row per send, with delivery status
//! (RFC 20260730-send-status S3).
//!
//! Distinct from the Sent conversation axis this will replace. That axis
//! lists conversations, and status is a property of an attempt: three
//! sends in one thread can be delivered, failed and retrying at once, and
//! a conversation row has nowhere to put that, nor anywhere to hang
//! "re-edit this one" when only one of the three failed.
//!
//! Nothing in the UI reads this yet. `:shadow` answers whether it is safe
//! to.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

use crate::handlers::kevy_util::with_kevy;
use mailrs_core_sidestate::families::send_read;

/// Read the stored RFC 5322 bytes for a send, or the reason there are none.
pub(crate) async fn envelope_bytes(user: &str, send_id: &str) -> Result<Vec<u8>, StatusCode> {
    let user_c = user.to_string();
    let send_c = send_id.to_string();
    let item = with_kevy(move |c| send_read::read_one(c, &user_c, &send_c))?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !item.can_resend() {
        // The maildir write failed at send time and `envelope_ref` records
        // the synthetic fallback rather than a path. There is nothing to
        // read, and 409 says so — a 500 would read as a transient fault
        // worth retrying, and an empty 200 would look like a message with
        // no content.
        return Err(StatusCode::CONFLICT);
    }
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let (path, id) =
        crate::handlers::messages::blob_ref_location(&maildir_root, user, &item.envelope_ref)
            .ok_or(StatusCode::NOT_FOUND)?;
    let store = mailrs_message_store::MaildirStore;
    use mailrs_message_store::MessageStore;
    store
        .fetch(&path, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
}

/// `GET /api/mail/sends/{send_id}:source` — the stored RFC 5322 bytes.
///
/// For downloading or inspecting a send exactly as it left. **Not** the
/// re-edit path: re-edit reads `:redraft` and the attachment bytes never
/// enter the browser (RFC 20260730-send-status S4 addendum).
pub async fn send_source(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Path(send_id): axum::extract::Path<String>,
) -> Result<axum::response::Response, StatusCode> {
    let bytes = envelope_bytes(&user, &send_id).await?;
    axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "message/rfc822")
        .body(axum::body::Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, serde::Serialize)]
pub struct ResendResponse {
    /// The new send's id. Distinct from the original: the envelope bytes
    /// are reused verbatim, so the Message-ID inside them is the same and
    /// cannot also be the key.
    pub send_id: String,
    pub resent_from: String,
}

/// `POST /api/mail/sends/{send_id}:resend`
///
/// Re-enqueues the stored envelope **unchanged**. A resend after failure
/// is the same message to someone who never received it, so rewriting its
/// Message-ID would make it a different message and lose the thread it
/// belongs to.
///
/// The original row is left exactly as it is. Flipping a `failed` row back
/// to `sending` would destroy the record of what happened, and that record
/// is the reason anyone opened this screen.
pub async fn resend(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Path(send_id): axum::extract::Path<String>,
) -> Result<Json<ResendResponse>, StatusCode> {
    let bytes = envelope_bytes(&user, &send_id).await?;

    let user_c = user.clone();
    let original = send_id.clone();
    let item = with_kevy(move |c| send_read::read_one(c, &user_c, &original))?
        .ok_or(StatusCode::NOT_FOUND)?;

    let recipients: Vec<String> = item
        .recipients
        .iter()
        .map(|r| r.recipient.clone())
        .collect();
    if recipients.is_empty() {
        return Err(StatusCode::CONFLICT);
    }

    let new_id = next_resend_id(&send_id, item.resent_from.as_deref());
    crate::handlers::prefs::enqueue_resend(
        &user,
        &recipients,
        &bytes,
        &new_id,
        &item.thread_id,
        &item.subject,
        &send_id,
    )?;
    Ok(Json(ResendResponse {
        send_id: new_id,
        resent_from: send_id,
    }))
}

/// Derive the next resend id from the original.
///
/// `<mid>` → `<mid>#r1` → `<mid>#r2`. Resending a resend chains off the
/// first send rather than nesting, so the ids stay readable and the
/// original is always the stem.
fn next_resend_id(send_id: &str, resent_from: Option<&str>) -> String {
    let stem = resent_from.unwrap_or(send_id);
    let stem = stem.split_once("#r").map_or(stem, |(s, _)| s);
    let n = send_id
        .split_once("#r")
        .and_then(|(_, n)| n.parse::<u32>().ok())
        .unwrap_or(0);
    format!("{stem}#r{}", n + 1)
}

#[cfg(test)]
mod tests {
    use super::next_resend_id;

    /// A resend must never reuse the original's id. The envelope bytes go
    /// out unchanged, so the Message-ID inside them is the same one — and
    /// if that were also the key, the resend would overwrite the failed
    /// row it exists to follow. The failure record is the reason the
    /// screen gets opened.
    #[test]
    fn a_resend_never_collides_with_the_send_it_repeats() {
        let first = next_resend_id("4974a5fd975d0dab@golia.jp", None);
        assert_eq!(first, "4974a5fd975d0dab@golia.jp#r1");
        assert_ne!(first, "4974a5fd975d0dab@golia.jp");
    }

    /// Resending a resend chains off the original rather than nesting, so
    /// ids stay readable and the first send is always the stem.
    #[test]
    fn resending_a_resend_increments_rather_than_nesting() {
        let second = next_resend_id(
            "4974a5fd975d0dab@golia.jp#r1",
            Some("4974a5fd975d0dab@golia.jp"),
        );
        assert_eq!(second, "4974a5fd975d0dab@golia.jp#r2");

        let third = next_resend_id(
            "4974a5fd975d0dab@golia.jp#r2",
            Some("4974a5fd975d0dab@golia.jp"),
        );
        assert_eq!(third, "4974a5fd975d0dab@golia.jp#r3");
    }

    /// Message-IDs contain `@` and dots and can contain almost anything
    /// else; only the `#r<n>` suffix is ours.
    #[test]
    fn an_awkward_message_id_survives() {
        let id = "a#b@weird.example.com";
        assert_eq!(next_resend_id(id, None), "a#b@weird.example.com#r1");
    }
}
