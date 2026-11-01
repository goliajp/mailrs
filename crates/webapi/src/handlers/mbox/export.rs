//! `GET /api/mail/export`: All Mail as one mbox, written as it is read.

use std::collections::HashSet;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};

use super::format::{Folder, write_message};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// Conversations asked for per page.
const PAGE: u32 = 200;

/// GET /api/mail/export — every message in All Mail (everything but Junk
/// and the Trash), each with its folder and read state, as mboxrd.
pub async fn export_mbox(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Response, StatusCode> {
    if !user.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    tokio::spawn(async move {
        if let Err(e) = write_all(&state, &user, &tx).await {
            tracing::warn!(%user, error = %e, "mbox export stopped");
        }
    });
    let body = Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|chunk| (Ok::<_, std::convert::Infallible>(chunk), rx))
    }));
    let name = format!(
        "attachment; filename=mailrs-export-{}.mbox",
        chrono::Utc::now().format("%Y%m%d")
    );
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/mbox".to_string(),
            ),
            (axum::http::header::CONTENT_DISPOSITION, name),
        ],
        body,
    )
        .into_response())
}

/// Page through All Mail by `before_ts`. A page boundary can fall inside a
/// run of conversations with the same timestamp, so each page asks for one
/// second more than the last row and skips what it already wrote.
async fn write_all(
    state: &WebState,
    user: &str,
    tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Result<(), String> {
    let mut done: HashSet<String> = HashSet::new();
    let mut before: Option<i64> = None;
    loop {
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                limit: PAGE,
                before_ts: before,
                folder: Some("all".into()),
                ..Default::default()
            },
        };
        let page = state
            .core
            .list_conversations(user, &req)
            .await
            .map_err(|e| e.to_string())?
            .items;
        let Some(oldest) = page.last().map(|c| c.last_date) else {
            return Ok(());
        };
        let mut fresh = 0;
        for conv in &page {
            if !done.insert(conv.thread_id.to_string()) {
                continue;
            }
            fresh += 1;
            let chunk = conversation(state, user, conv).await?;
            if tx.send(chunk).await.is_err() {
                // the client went away
                return Ok(());
            }
        }
        before = match fresh {
            // a whole page at one timestamp: step past it
            0 => Some(oldest),
            _ => Some(oldest + 1),
        };
        if page.len() < PAGE as usize && fresh == 0 {
            return Ok(());
        }
    }
}

async fn conversation(
    state: &WebState,
    user: &str,
    conv: &ConversationSummaryWire,
) -> Result<Vec<u8>, String> {
    let messages = state
        .core
        .list_thread_messages(user, &conv.thread_id)
        .await
        .map_err(|e| e.to_string())?
        .items;
    let mut out = Vec::new();
    for m in messages {
        let Ok(raw) = crate::handlers::messages::read_maildir_bytes_pub(user, &m.blob_ref).await
        else {
            tracing::warn!(%user, uid = m.uid, "mbox export: message file missing, skipped");
            continue;
        };
        let folder = folder_of(user, &m.sender, conv.archived);
        let seen = m.flags & mailrs_core_api::method::message::FLAG_SEEN != 0;
        write_message(&mut out, &raw, m.internal_date, folder, seen);
    }
    Ok(out)
}

/// What the user wrote is Sent; the rest follows the conversation.
fn folder_of(user: &str, from_header: &str, archived: bool) -> Folder {
    let addr = match from_header.rsplit_once('<') {
        Some((_, rest)) => rest.trim_end_matches('>').trim(),
        None => from_header.trim(),
    };
    if addr.eq_ignore_ascii_case(user) {
        return Folder::Sent;
    }
    match archived {
        true => Folder::Archive,
        false => Folder::Inbox,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_users_own_mail_is_sent_whatever_the_conversation() {
        let u = "me@x.jp";
        assert_eq!(folder_of(u, "Me <ME@x.jp>", true), Folder::Sent);
        assert_eq!(folder_of(u, "me@x.jp", false), Folder::Sent);
        assert_eq!(folder_of(u, "A <a@y>", true), Folder::Archive);
        assert_eq!(folder_of(u, "a@y", false), Folder::Inbox);
    }
}
