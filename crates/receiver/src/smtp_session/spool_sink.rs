//! The receiver's spool-write seam (P6 receiver/core split).
//!
//! In the split, the DATA handler writes the accepted message (antispam
//! already run) to the spool via this trait instead of resolving recipients
//! and delivering — the receiver has no spg, so resolve/sieve/relay/quota all
//! move to the core consumer. The concrete impl (a maildir over the spool
//! root) is constructed by the receiver binary; the monolith leaves
//! `ConnectionContext::spool_sink` as `None` and keeps the inline delivery
//! path unchanged.

use async_trait::async_trait;

use mailrs_message_store::{MaildirStore, MessageStore};

#[async_trait]
pub trait SpoolSink: Send + Sync {
    /// Persist an encoded spool blob (envelope header + body) atomically and
    /// return the spool id (the maildir filename) for the `SpoolDelivered`
    /// notify and for the core to fetch by.
    async fn write(&self, blob: &[u8]) -> std::io::Result<String>;
}

/// The default spool sink: a maildir at `{spool_root}/incoming` shared with
/// the core. `deliver_batch` is atomic (tmp→new rename), so a single write is
/// one durable spool file. The core scans this same dir (notify + reconcile)
/// and fetches by the returned id.
pub struct MaildirSpoolSink {
    store: MaildirStore,
    incoming_path: String,
}

impl MaildirSpoolSink {
    /// `spool_root` is the spool maildir root (e.g. `{maildir_root}/.spool`);
    /// messages land in its `incoming` mailbox.
    pub fn new(spool_root: &str) -> Self {
        Self {
            store: MaildirStore,
            incoming_path: format!("{spool_root}/incoming"),
        }
    }
}

#[async_trait]
impl SpoolSink for MaildirSpoolSink {
    async fn write(&self, blob: &[u8]) -> std::io::Result<String> {
        let ids = self
            .store
            .deliver_batch(&self.incoming_path, &[blob])
            .await?;
        ids.into_iter()
            .next()
            .map(|id| id.to_string())
            .ok_or_else(|| std::io::Error::other("spool deliver_batch returned no id"))
    }
}
