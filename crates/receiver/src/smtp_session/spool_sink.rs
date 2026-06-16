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

#[async_trait]
pub trait SpoolSink: Send + Sync {
    /// Persist an encoded spool blob (envelope header + body) atomically and
    /// return the spool id (the maildir filename) for the `SpoolDelivered`
    /// notify and for the core to fetch by.
    async fn write(&self, blob: &[u8]) -> std::io::Result<String>;
}
