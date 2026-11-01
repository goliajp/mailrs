//! `POST /api/mail/import`: an mbox upload, read as it arrives and filed
//! message by message by the core.
//!
//! What the user wrote goes in as a sent copy; everything else into INBOX,
//! then read and archived as the file says. Junk and the Trash of another
//! mailbox are left behind, and a Message-ID already in this mailbox is
//! not filed twice, so an import can be run again after it stops.

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use futures_util::StreamExt as _;
use mailrs_core_api::method::admin::SetMessageFlagsRequest;
use mailrs_core_api::method::message::FLAG_SEEN;

use super::format::{Folder, Parsed, Splitter};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// What one upload did, message by message.
#[derive(Debug, Default, serde::Serialize, PartialEq, Eq)]
pub struct Report {
    /// Filed into INBOX (archived or not).
    pub imported: u64,
    /// Filed as sent copies.
    pub sent: u64,
    /// Of `imported`, archived afterwards.
    pub archived: u64,
    pub skipped_duplicate: u64,
    pub skipped_junk_or_trash: u64,
    /// Larger than a core files.
    pub too_large: u64,
    pub failed: u64,
    /// False when the upload ended before the file did.
    pub complete: bool,
}

/// POST /api/mail/import — the request body is the mbox itself.
pub async fn import_mbox(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    body: Body,
) -> Result<Json<Report>, StatusCode> {
    if !user.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut report = Report::default();
    let mut splitter = Splitter::default();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            tracing::warn!(%user, "mbox import: the upload ended early");
            return Ok(Json(report));
        };
        for m in splitter.push(&chunk) {
            file_one(&state, &user, m, &mut report).await;
        }
    }
    for m in splitter.finish() {
        file_one(&state, &user, m, &mut report).await;
    }
    report.complete = true;
    tracing::info!(%user, ?report, "mbox import");
    Ok(Json(report))
}

async fn file_one(state: &WebState, user: &str, m: Parsed, report: &mut Report) {
    if m.folder == Folder::Discarded {
        report.skipped_junk_or_trash += 1;
        return;
    }
    if m.raw.len() > mailrs_core_api::method::sent::MAX_MESSAGE_BYTES {
        report.too_large += 1;
        return;
    }
    let message = mailrs_rfc5322::Message::new(&m.raw);
    let message_id = message
        .header_str("Message-ID")
        .map(|v| {
            v.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
        .unwrap_or_default();
    if !message_id.is_empty()
        && state
            .core
            .find_by_message_id_for_user(user, &message_id)
            .await
            .is_ok()
    {
        report.skipped_duplicate += 1;
        return;
    }
    let result = match m.folder {
        Folder::Sent => {
            let rcpts = recipients(&message);
            state
                .core
                .file_sent(user, &m.raw, &rcpts)
                .await
                .map(|_| report.sent += 1)
        }
        _ => into_inbox(state, user, &m, report).await,
    };
    if let Err(e) = result {
        tracing::warn!(%user, error = %e, "mbox import: a message was not filed");
        report.failed += 1;
    }
}

async fn into_inbox(
    state: &WebState,
    user: &str,
    m: &Parsed,
    report: &mut Report,
) -> Result<(), mailrs_core_api::error::CoreApiError> {
    let filed = state.core.import_message(user, &m.raw).await?;
    report.imported += 1;
    if m.seen {
        let wire = state
            .core
            .find_by_message_id_for_user(user, &filed.message_id)
            .await?;
        let req = SetMessageFlagsRequest {
            flags: wire.flags | FLAG_SEEN,
        };
        state.core.set_message_flags(user, wire.uid, &req).await?;
    }
    if m.folder == Folder::Archive {
        state.core.archive_thread(user, &filed.thread_id).await?;
        report.archived += 1;
    }
    Ok(())
}

/// To and Cc, as written.
fn recipients(message: &mailrs_rfc5322::Message<'_>) -> Vec<String> {
    ["To", "Cc"]
        .into_iter()
        .filter_map(|h| message.header_str(h))
        .flat_map(mailrs_rfc5322::split_list)
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipients_are_to_and_cc_as_written() {
        let raw = b"To: A <a@x>, b@y\r\nCc: \"C, D\" <c@z>\r\nSubject: s\r\n\r\nb\r\n";
        let m = mailrs_rfc5322::Message::new(raw);
        assert_eq!(recipients(&m), vec!["A <a@x>", "b@y", "\"C, D\" <c@z>"]);
    }
}
