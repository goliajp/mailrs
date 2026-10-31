//! A rule's actions, done to one conversation as its mail arrives — the
//! same state changes the kevy core makes, and like it nothing trained.

use mailrs_mailbox::rules::Action;
use mailrs_mailbox::scope::Bucket;

use super::PgCore;

impl PgCore {
    pub(super) async fn apply_rules(&self, user: &str, thread_id: &str, actions: &[Action]) {
        for action in actions {
            let done = match action {
                Action::Label(id) => self
                    .mailbox
                    .set_label(user, id, thread_id, true)
                    .await
                    .map_err(|e| format!("{e:?}")),
                Action::Archive => self
                    .mailbox
                    .archive_thread(user, thread_id)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Action::Read => self
                    .mailbox
                    .mark_thread_read(user, thread_id, None)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Action::Star => self
                    .mailbox
                    .star_thread(user, thread_id)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Action::Junk => self
                    .mailbox
                    .file_thread(user, thread_id, Bucket::Junk)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Action::Trash => self
                    .mailbox
                    .trash_thread(user, thread_id, chrono::Utc::now().timestamp())
                    .await
                    .map_err(|e| e.to_string()),
            };
            if let Err(e) = done {
                tracing::warn!(error = %e, %user, %thread_id, action = %action.spelling(), "rule action failed");
            }
        }
    }
}
