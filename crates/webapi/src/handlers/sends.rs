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
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

use mailrs_core_sidestate::families::send::Status;
use mailrs_core_sidestate::families::send_read;

fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, serde::Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    /// One of the status names. An unrecognised value is rejected rather
    /// than quietly widened to "everything" — a filter that silently
    /// stops filtering shows the user more than they asked for and looks
    /// like it worked.
    pub status: Option<String>,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, serde::Serialize)]
pub struct RecipientResponse {
    pub recipient: String,
    pub delivered: bool,
    pub pending: bool,
    /// The remote's reply code, or 0 before any server answered.
    pub code: u16,
    /// The remote's text, verbatim.
    pub message: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SendResponse {
    pub send_id: String,
    pub thread_id: String,
    pub subject: String,
    pub to: Vec<String>,
    pub created_at: i64,
    pub status: String,
    /// Whether resend / re-edit have envelope bytes to work from. The UI
    /// needs this to decide whether to offer the buttons at all, rather
    /// than offering them and doing nothing.
    pub can_resend: bool,
    pub resent_from: Option<String>,
    pub recipients: Vec<RecipientResponse>,
}

impl From<send_read::SendListItem> for SendResponse {
    fn from(i: send_read::SendListItem) -> Self {
        let can_resend = i.can_resend();
        Self {
            send_id: i.send_id,
            thread_id: i.thread_id,
            subject: i.subject,
            to: i
                .to_csv
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            created_at: i.created_at,
            status: i.status.as_str().to_string(),
            can_resend,
            resent_from: i.resent_from,
            recipients: i
                .recipients
                .into_iter()
                .map(|r| RecipientResponse {
                    recipient: r.recipient,
                    delivered: r.delivered,
                    pending: r.pending,
                    code: r.code,
                    message: r.message,
                })
                .collect(),
        }
    }
}

/// `GET /api/mail/sends?limit=&offset=&status=`
pub async fn list_sends(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SendResponse>>, StatusCode> {
    let status = match q.status.as_deref() {
        None | Some("") => None,
        Some(s) => Some(Status::parse(s).ok_or(StatusCode::BAD_REQUEST)?),
    };
    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);
    let items = with_kevy(move |c| send_read::list_sends(c, &user, status, offset, limit))?;
    Ok(Json(items.into_iter().map(Into::into).collect()))
}

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
mod resend_id_tests {
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

#[derive(Debug, serde::Deserialize)]
pub struct ShadowQuery {
    /// Sends at or after this epoch-second must have a row; anything
    /// earlier predates the row-writing and is expected to be missing.
    /// Required, because a default would decide the cutover silently and
    /// the answer depends entirely on it.
    pub since: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ShadowResponse {
    pub axis_threads: u64,
    pub projection_sends: u64,
    pub missing_before: u64,
    /// The gate. Must be zero before the Send view reads the projection.
    pub missing_since: u64,
    pub samples: Vec<String>,
}

/// `POST /api/mail/sends:shadow?since=<epoch>`
///
/// Compares the projection against the threads the old Sent axis holds.
/// Split by time on purpose: every send made before the row-writing
/// shipped has no row and always will, so counting those alongside a live
/// regression would bury it — the thread-counter work nearly lost 182
/// actionable rows inside 64 expected ones by summing them.
pub async fn shadow_sends(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ShadowQuery>,
) -> Result<Json<ShadowResponse>, StatusCode> {
    // The old axis lives behind the core, which owns the embedded store.
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: 2000,
            before_ts: None,
            category: None,
            domains: None,
            archived: false,
            folder: Some("Sent".to_string()),
            unread: None,
            starred: None,
            section: None,
        },
    };
    let axis: Vec<(String, i64)> = state
        .core
        .list_conversations(&user, &req)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .items
        .into_iter()
        .map(|c| (c.thread_id, c.last_date))
        .collect();

    let since = q.since;
    let user_c = user.clone();
    let r = with_kevy(move |c| send_read::shadow_report(c, &user_c, &axis, since))?;
    if r.missing_since > 0 {
        tracing::warn!(
            %user, missing_since = r.missing_since, samples = ?r.samples,
            "sends since the cutover with no Send row"
        );
    }
    Ok(Json(ShadowResponse {
        axis_threads: r.axis_threads,
        projection_sends: r.projection_sends,
        missing_before: r.missing_before,
        missing_since: r.missing_since,
        samples: r.samples,
    }))
}

/// A failed send as compose fields, ready to reopen in the composer.
#[derive(Debug, serde::Serialize)]
pub struct RedraftResponse {
    pub redraft_of: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub html_body: String,
    pub in_reply_to: Option<String>,
    /// The carried attachments, described but not transferred. `index` is
    /// what a later send passes back in `redraft_keep`.
    pub attachments: Vec<RedraftAttachment>,
}

#[derive(Debug, serde::Serialize)]
pub struct RedraftAttachment {
    pub index: usize,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
}

/// `GET /api/mail/sends/{send_id}:redraft` — a failed send as compose
/// fields, so the user can fix what went wrong and send again.
///
/// The attachment bytes stay here. Only their names and sizes go to the
/// browser; the send that follows names the ones to keep by index and the
/// server re-extracts them. Re-editing a 15 MB mail therefore costs no
/// transfer, and cannot lose the files — which is what would happen if
/// re-edit went through the drafts table, since a draft has no
/// attachments field at all.
///
/// The metadata comes from the same `attachments_from_envelope` walk the
/// send path uses. That is the point: an index means the same part on
/// both sides because one function decides what the parts are.
pub async fn send_redraft(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Path(send_id): axum::extract::Path<String>,
) -> Result<Json<RedraftResponse>, StatusCode> {
    let user_c = user.clone();
    let send_c = send_id.clone();
    let item = with_kevy(move |c| send_read::read_one(c, &user_c, &send_c))?
        .ok_or(StatusCode::NOT_FOUND)?;
    let bytes = envelope_bytes(&user, &send_id).await?;

    let (text, html, _) = crate::handlers::conversations::parse_body(&bytes);
    let to = split_csv(&item.to_csv);
    let cc = split_csv(&item.cc_csv);
    let all: Vec<String> = item
        .recipients
        .iter()
        .map(|r| r.recipient.clone())
        .collect();
    let bcc = bcc_from(&all, &to, &cc);

    let attachments = crate::handlers::prefs::attachments_from_envelope(&bytes)
        .into_iter()
        .enumerate()
        .map(|(index, a)| RedraftAttachment {
            index,
            filename: a.filename,
            content_type: a.content_type,
            size: a.bytes.len(),
        })
        .collect();

    Ok(Json(RedraftResponse {
        redraft_of: send_id,
        to,
        cc,
        bcc,
        subject: item.subject,
        body: text.unwrap_or_default(),
        html_body: html.unwrap_or_default(),
        // Staying in the thread it was addressed to. A repair of a failed
        // send belongs where the original was going, not in a new
        // conversation.
        in_reply_to: Some(item.thread_id).filter(|t| !t.is_empty()),
        attachments,
    }))
}

/// The Bcc set: every queued recipient that is not in To or Cc.
///
/// A Bcc header is not in the envelope — it would not be blind — so this
/// is the only way to put a blind recipient back in the right field on
/// re-edit. Getting it wrong moves someone from Bcc into a visible
/// header, which is a disclosure, not a display bug.
///
/// Matching is on the whole address, lowercased. Not `contains`: `a@b.com`
/// is a substring of `xa@b.com`, and the codebase already carries one live
/// bug of exactly that shape in `senders_csv_contains_user`.
fn bcc_from(recipients: &[String], to: &[String], cc: &[String]) -> Vec<String> {
    let addressed: std::collections::HashSet<String> = to
        .iter()
        .chain(cc.iter())
        .map(|s| s.trim().to_lowercase())
        .collect();
    recipients
        .iter()
        .filter(|r| !addressed.contains(&r.trim().to_lowercase()))
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect()
}

/// Split a stored `to_csv` / `cc_csv` back into addresses.
fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod redraft_tests {
    use super::{bcc_from, split_csv};

    /// Bcc is reconstructed, not stored, so an error here moves a blind
    /// recipient into a visible header on the next send. That is a
    /// disclosure.
    #[test]
    fn a_blind_recipient_stays_blind_when_re_edited() {
        let to = vec!["visible@x.com".to_string()];
        let cc = vec!["copied@x.com".to_string()];
        let all = vec![
            "visible@x.com".to_string(),
            "copied@x.com".to_string(),
            "blind@x.com".to_string(),
        ];
        assert_eq!(bcc_from(&all, &to, &cc), vec!["blind@x.com".to_string()]);
    }

    /// The recipients list is what the queue was handed; To and Cc come
    /// from headers, which carry whatever case the sender typed.
    #[test]
    fn case_does_not_turn_a_visible_recipient_into_a_bcc() {
        let to = vec!["Visible@X.com".to_string()];
        let all = vec!["visible@x.com".to_string()];
        assert!(
            bcc_from(&all, &to, &[]).is_empty(),
            "the same address in different case is the same recipient"
        );
    }

    /// `a@b.com` is a substring of `xa@b.com`. Matching on containment
    /// rather than equality would drop a real Bcc recipient, and the
    /// codebase already carries one live bug of that shape
    /// (`senders_csv_contains_user`).
    #[test]
    fn a_longer_address_that_ends_with_a_visible_one_is_still_a_bcc() {
        let to = vec!["a@b.com".to_string()];
        let all = vec!["a@b.com".to_string(), "xa@b.com".to_string()];
        assert_eq!(bcc_from(&all, &to, &[]), vec!["xa@b.com".to_string()]);
    }

    #[test]
    fn csv_splitting_tolerates_the_spacing_a_header_carries() {
        assert_eq!(
            split_csv("a@x.com, b@x.com ,, c@x.com"),
            vec![
                "a@x.com".to_string(),
                "b@x.com".to_string(),
                "c@x.com".to_string()
            ]
        );
        assert!(split_csv("").is_empty());
    }
}
