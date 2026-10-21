//! A copy that reached one of this core's mailboxes is indexed here; the
//! report hooks, sieve and the maildir write before it are shared
//! (`mailrs_intake`).

use mailrs_intake::Core;

use super::core::Fastcore;

impl Core for Fastcore<'_> {
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
        crate::ingest_delivered_file(self.state, addr, blob_ref, body, folder, fraud_verdict);
        true
    }
}
