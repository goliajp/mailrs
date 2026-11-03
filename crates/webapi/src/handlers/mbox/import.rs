//! `POST /api/mail/import`: an mbox upload, read as it arrives and filed
//! message by message by the core.
//!
//! What the user wrote goes in as a sent copy; everything else into INBOX,
//! then read and archived as the file says. Junk and the Trash of another
//! mailbox are left behind, and a Message-ID already in this mailbox is
//! not filed twice, so an import can be run again after it stops.
//!
//! Received mail is judged and screened on the way in ([`super::judge`]),
//! the way the receiver judges what arrives over SMTP.

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use futures_util::StreamExt as _;
use mailrs_core_api::method::admin::SetMessageFlagsRequest;
use mailrs_core_api::method::message::FLAG_SEEN;

use super::format::{Folder, Parsed, Splitter};
use super::judge::Judge;
use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// What one import did, message by message.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Report {
    /// Filed into INBOX (archived or not).
    pub imported: u64,
    /// Filed as sent copies.
    pub sent: u64,
    /// Of `imported`, archived afterwards.
    pub archived: u64,
    /// Of `imported`, held as suspected fraud, as a delivery would be.
    #[serde(default)]
    pub held: u64,
    pub skipped_duplicate: u64,
    pub skipped_junk_or_trash: u64,
    /// Larger than a core files.
    pub too_large: u64,
    pub failed: u64,
    /// The mailbox reached its quota; nothing after that was filed.
    #[serde(default)]
    pub over_quota: bool,
    /// False when the import ended before the source did.
    pub complete: bool,
}

/// Files messages for one user, one at a time, counting what happened.
pub struct Filer {
    state: Arc<WebState>,
    user: String,
    judge: Arc<Mutex<Judge>>,
    pub report: Report,
}

impl Filer {
    /// Set up the checks for `user`'s import.
    pub async fn new(state: Arc<WebState>, user: String) -> Self {
        // the accounts' names are part of the fraud policy; without them the
        // check for somebody wearing a colleague's name cannot fire
        let names = match state.core.list_accounts().await {
            Ok(list) => list
                .items
                .into_iter()
                .map(|a| a.display_name.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "mbox import: account names unavailable to the fraud policy");
                Vec::new()
            }
        };
        let judge = tokio::task::spawn_blocking(move || Judge::new(names))
            .await
            .expect("setting up the import checks panicked");
        Self {
            state,
            user,
            judge: Arc::new(Mutex::new(judge)),
            report: Report::default(),
        }
    }

    /// File one message. False once the mailbox is over quota: stop then.
    pub async fn file(&mut self, m: Parsed) -> bool {
        if m.folder == Folder::Discarded {
            self.report.skipped_junk_or_trash += 1;
            return true;
        }
        if m.raw.len() > mailrs_core_api::method::sent::MAX_MESSAGE_BYTES {
            self.report.too_large += 1;
            return true;
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
            && self
                .state
                .core
                .find_by_message_id_for_user(&self.user, &message_id)
                .await
                .is_ok()
        {
            self.report.skipped_duplicate += 1;
            return true;
        }
        let result = match m.folder {
            Folder::Sent => {
                let rcpts = recipients(&message);
                self.state
                    .core
                    .file_imported_sent(&self.user, &m.raw, &rcpts)
                    .await
                    .map(|_| self.report.sent += 1)
            }
            _ => self.file_received(&m).await,
        };
        match result {
            Ok(()) => true,
            Err(e) if e.status_code() == 507 => {
                self.report.over_quota = true;
                false
            }
            Err(e) => {
                tracing::warn!(user = %self.user, error = %e, "mbox import: a message was not filed");
                self.report.failed += 1;
                true
            }
        }
    }

    async fn file_received(
        &mut self,
        m: &Parsed,
    ) -> Result<(), mailrs_core_api::error::CoreApiError> {
        let (judge, user, raw) = (self.judge.clone(), self.user.clone(), m.raw.clone());
        let checked = tokio::task::spawn_blocking(move || {
            judge.lock().expect("import checks lock").check(&user, &raw)
        })
        .await
        .expect("the import checks panicked");
        let filed = self
            .state
            .core
            .import_message(&self.user, &m.raw, checked.fraud_verdict)
            .await?;
        self.report.imported += 1;
        self.report.held += u64::from(checked.holds);
        if m.seen {
            let wire = self
                .state
                .core
                .find_by_message_id_for_user(&self.user, &filed.message_id)
                .await?;
            let req = SetMessageFlagsRequest {
                flags: wire.flags | FLAG_SEEN,
            };
            self.state
                .core
                .set_message_flags(&self.user, wire.uid, &req)
                .await?;
        }
        if m.folder == Folder::Archive {
            self.state
                .core
                .archive_thread(&self.user, &filed.thread_id)
                .await?;
            self.report.archived += 1;
        }
        Ok(())
    }
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
    let mut filer = Filer::new(state, user.clone()).await;
    let mut splitter = Splitter::default();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            tracing::warn!(%user, "mbox import: the upload ended early");
            return Ok(Json(filer.report));
        };
        for m in splitter.push(&chunk) {
            if !filer.file(m).await {
                return Ok(Json(filer.report));
            }
        }
    }
    for m in splitter.finish() {
        if !filer.file(m).await {
            return Ok(Json(filer.report));
        }
    }
    filer.report.complete = true;
    tracing::info!(%user, report = ?filer.report, "mbox import");
    Ok(Json(filer.report))
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
