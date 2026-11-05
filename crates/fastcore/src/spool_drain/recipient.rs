//! A copy that reached one of this core's mailboxes is indexed here; the
//! report hooks, sieve and the maildir write before it are shared
//! (`mailrs_intake`).

use mailrs_intake::Core;

use super::core::Fastcore;

impl Core for Fastcore {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        self.state.net_conn()
    }

    /// Always taken: a write this store cannot make is logged where it
    /// happens, and the maildir self-heal indexes the file later.
    fn ingest(
        &self,
        addr: &str,
        blob_ref: &str,
        body: &[u8],
        folder: &str,
        fraud_verdict: Option<&str>,
    ) -> bool {
        crate::ingest_delivered_file(&self.state, addr, blob_ref, body, folder, fraud_verdict);
        true
    }

    fn tag_shared(
        &self,
        addr: &str,
        message_id: &str,
        tag: &str,
        assignee: &str,
    ) -> Option<String> {
        let mailbox = &self.state.mailbox;
        let tid = mailbox
            .thread_for_message_id(addr, message_id)
            .ok()
            .flatten()?;
        match mailbox.set_shared_copy(addr, &tid, tag, assignee) {
            Ok(()) => Some(tid),
            Err(e) => {
                tracing::warn!(%addr, %tag, error = %e, "team inbox: copy not tagged");
                None
            }
        }
    }

    fn may_send_as(&self, user: &str, from: &str) -> bool {
        crate::routes::may_send_as_blocking(&self.state, user, from)
    }

    fn file_submitted(
        &self,
        user: &str,
        body: &[u8],
        recipients: &[String],
    ) -> std::io::Result<()> {
        let store = crate::sent::Store(self.state.clone());
        mailrs_intake::sent::file_submitted(&store, user, body, recipients).map(|_| ())
    }
}
