//! This core as the intake flows see it: filing a sent, imported or
//! delivered copy (`mailrs_intake::sent`) and reporting a scam
//! (`report::fingerprint`). The routes are `mailrs_core_routes::intake`.

use std::io;
use std::sync::Arc;

use mailrs_intake::arrival::{Arrival, Threads};
use mailrs_intake::sent::SentStore;

use crate::FastcoreState;

pub struct Store(pub(crate) Arc<FastcoreState>);

impl mailrs_core_routes::store::IntakeStore for FastcoreState {
    type Core = Store;

    fn intake_core(self: &Arc<Self>) -> Store {
        Store(self.clone())
    }
}

impl Threads for Store {
    fn thread_for_message_id(&self, user: &str, message_id: &str) -> Option<String> {
        crate::headers::Threads(&self.0).thread_for_message_id(user, message_id)
    }
    fn thread_subject(&self, user: &str, thread_id: &str) -> Option<String> {
        crate::headers::Threads(&self.0).thread_subject(user, thread_id)
    }
    fn reader_wrote_in(&self, user: &str, thread_id: &str) -> bool {
        crate::headers::Threads(&self.0).reader_wrote_in(user, thread_id)
    }
}

impl SentStore for Store {
    fn maildir_root(&self) -> &str {
        &self.0.maildir_root
    }
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        self.0.net_conn()
    }
    fn authserv_id(&self) -> Option<&str> {
        self.0.authserv_id.as_deref()
    }
    /// Written as the drain writes an arrival the user wrote; a write this
    /// store cannot make is logged there, and the maildir self-heal indexes
    /// the file later, as for any delivered file.
    fn index_own(
        &self,
        user: &str,
        blob_ref: &str,
        body: &[u8],
        a: &Arrival<'_>,
    ) -> io::Result<()> {
        crate::ingest::store_arrival(&self.0, user, blob_ref, body, a, None);
        Ok(())
    }
    fn index_arrival(
        &self,
        user: &str,
        blob_ref: &str,
        body: &[u8],
        a: &Arrival<'_>,
        fraud_verdict: Option<&str>,
    ) -> io::Result<()> {
        crate::ingest::store_arrival(&self.0, user, blob_ref, body, a, fraud_verdict);
        Ok(())
    }
}
