//! The sender's own copy of a message they sent, written to this store the
//! way an arriving message the user wrote is (`mailrs_intake::sent`).

use std::io;

use mailrs_intake::arrival::Arrival;
use mailrs_intake::sent::SentStore;

use super::PgCore;

impl SentStore for PgCore {
    fn maildir_root(&self) -> &str {
        &self.dir.maildir_root
    }

    fn net_conn(&self) -> Option<kevy_client::Connection> {
        mailrs_intake::Core::net_conn(self)
    }

    fn authserv_id(&self) -> Option<&str> {
        self.dir.authserv_id.as_deref()
    }

    fn index_own(
        &self,
        user: &str,
        blob_ref: &str,
        body: &[u8],
        a: &Arrival<'_>,
    ) -> io::Result<()> {
        let mailbox = super::ingest::mailbox_for(true, "Sent");
        self.dir
            .rt
            .block_on(self.store(user, mailbox, blob_ref, body, a, None, None))
            .map_err(io::Error::other)?;
        self.announce(user, a);
        Ok(())
    }

    /// Into INBOX, where `mailrs_intake::import` wrote it, whoever wrote it.
    fn index_arrival(
        &self,
        user: &str,
        blob_ref: &str,
        body: &[u8],
        a: &Arrival<'_>,
        fraud_verdict: Option<&str>,
    ) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.store(user, "INBOX", blob_ref, body, a, None, fraud_verdict))
            .map_err(io::Error::other)?;
        Ok(())
    }
}
