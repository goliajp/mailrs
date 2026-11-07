//! Where the run over the attachment-text backlog stands: one document,
//! written whole, so the run can stop and pick up where it left off after a
//! restart. What the document says is fastcore's; this only keeps it.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// The backlog run's saved progress; `None` before the first run.
    pub fn attachment_backlog(&self) -> io::Result<Option<String>> {
        Ok(self
            .store()
            .hget(keys::ATTACHMENT_BACKLOG, keys::ATTACHMENT_BACKLOG_FIELD)?
            .map(|raw| String::from_utf8_lossy(&raw).into_owned()))
    }

    /// Replace the backlog run's saved progress.
    pub fn set_attachment_backlog(&self, progress: &str) -> io::Result<()> {
        self.store().hset(
            keys::ATTACHMENT_BACKLOG,
            &[(keys::ATTACHMENT_BACKLOG_FIELD, progress.as_bytes())],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kevy_embedded::{Config, Store};

    use crate::KevyMailboxStore;

    #[test]
    fn progress_reads_back_what_was_saved_and_nothing_before() {
        let store = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        assert_eq!(store.attachment_backlog().unwrap(), None);
        store
            .set_attachment_backlog(r#"{"state":"running"}"#)
            .unwrap();
        store.set_attachment_backlog(r#"{"state":"done"}"#).unwrap();
        assert_eq!(
            store.attachment_backlog().unwrap().as_deref(),
            Some(r#"{"state":"done"}"#)
        );
    }
}
