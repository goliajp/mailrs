//! A file delivered into one of this store's maildirs, read once by the
//! shared steps (`mailrs_intake::arrival`, `effects`, `importance`) and
//! written here.

use mailrs_intake::arrival::{Arrival, Threads, derive};
use mailrs_intake::{Core, effects};
use mailrs_intelligence::importance::ImportanceLevel;

use super::PgCore;
use crate::event_bus::SmtpEvent;

impl Threads for PgCore {
    fn thread_for_message_id(&self, user: &str, message_id: &str) -> Option<String> {
        self.dir
            .rt
            .block_on(self.mailbox.find_thread_id_by_message_id(user, message_id))
            .ok()
            .flatten()
    }

    fn thread_subject(&self, user: &str, thread_id: &str) -> Option<String> {
        self.dir
            .rt
            .block_on(self.mailbox.thread_display_subject(user, thread_id))
            .ok()
            .flatten()
    }
}

/// The mailbox a copy is indexed into. Mail the user wrote is in Sent, as
/// this store's views expect; the receiver's Junk verdict is a category,
/// not a mailbox, as in the kevy core.
pub(super) fn mailbox_for(is_own: bool, folder: &str) -> &str {
    if is_own {
        return "Sent";
    }
    if folder.eq_ignore_ascii_case("INBOX") || folder.eq_ignore_ascii_case("Junk") {
        return "INBOX";
    }
    folder
}

impl PgCore {
    /// Index `body`, just written to `addr`'s maildir as `blob_ref` (the
    /// file name, with its subfolder when filed into `folder`). Returns
    /// whether the store took it; when it did not, the spool file stays and
    /// the copy is delivered again on a later tick.
    pub(super) fn index(
        &self,
        addr: &str,
        blob_ref: &str,
        body: &[u8],
        folder: &str,
        fraud_verdict: Option<&str>,
    ) -> bool {
        let a = derive(self, self.net_conn(), addr, blob_ref, body, folder);
        // importance follows the latest inbound message; the user's own
        // reply must not restate it
        let importance = match a.is_own {
            true => None,
            false => mailrs_intake::importance::score_inbound(
                self.net_conn(),
                addr,
                &a.from,
                a.head,
                body,
            ),
        };
        effects::before_store(self.net_conn(), addr, &a, body.len());
        let mailbox = mailbox_for(a.is_own, folder);
        let stored = self.dir.rt.block_on(self.store(
            addr,
            mailbox,
            blob_ref,
            body,
            &a,
            importance,
            fraud_verdict,
        ));
        let held = match stored {
            Ok(held) => held,
            Err(e) => {
                tracing::warn!(error = %e, %addr, message_id = %a.message_id, "indexing a delivered message failed");
                return false;
            }
        };
        effects::after_store(addr, &a, held);
        self.announce(addr, &a);
        true
    }

    /// The in-process event IMAP IDLE and the cache listen for.
    pub(super) fn announce(&self, addr: &str, a: &Arrival<'_>) {
        self.event_bus.emit(SmtpEvent::NewMessage {
            user: addr.to_string(),
            thread_id: a.thread_id.clone(),
            sender: a.from.clone(),
            subject: a.subject.clone(),
            snippet: a.preview.clone(),
        });
    }
}

/// The importance columns: empty for mail that was not scored.
pub(super) fn importance_columns(v: Option<(ImportanceLevel, f32)>) -> (&'static str, f32) {
    match v {
        Some((level, score)) => (level.as_str(), score),
        None => ("", 0.0),
    }
}
