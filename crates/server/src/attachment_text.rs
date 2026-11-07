//! Attachment text in this core: the shared worker
//! (`mailrs_intake::attachment_text`) over the SQL store, which keeps the
//! queue in tables and the outcome on every copy of a message.

use std::io;
use std::sync::Arc;

use mailrs_intake::attachment_text::{AttachmentTextStore, TEXT_CAP, cap_text, kept_qr_urls};
use mailrs_mailbox::{AttachmentTextState, PgMailboxStore};

/// This core's [`AttachmentTextStore`].
pub struct PgAttachmentText {
    pub mailbox: Arc<PgMailboxStore>,
    pub maildir_root: String,
    /// The network kevy the number histories live in.
    pub net_url: Option<String>,
}

fn io_err(e: sqlx::Error) -> io::Error {
    io::Error::other(e)
}

impl AttachmentTextStore for PgAttachmentText {
    async fn queue(&self, user: &str, mid: &str) -> io::Result<()> {
        self.mailbox
            .queue_attachment_text(user, mid)
            .await
            .map_err(io_err)
    }
    async fn pending(&self, limit: usize) -> io::Result<Vec<(String, String)>> {
        self.mailbox
            .pending_attachment_texts(limit)
            .await
            .map_err(io_err)
    }
    async fn begin(&self, user: &str, mid: &str) -> io::Result<u32> {
        self.mailbox
            .begin_attachment_text(user, mid)
            .await
            .map_err(io_err)
    }
    async fn dequeue(&self, user: &str, mid: &str) -> io::Result<()> {
        self.mailbox
            .dequeue_attachment_text(user, mid)
            .await
            .map_err(io_err)
    }
    async fn set_aside(&self, user: &str, mid: &str) -> io::Result<()> {
        self.mailbox
            .set_aside_attachment_text(user, mid)
            .await
            .map_err(io_err)
    }
    async fn queued(&self, user: &str, mid: &str) -> io::Result<bool> {
        self.mailbox
            .attachment_text_queued(user, mid)
            .await
            .map_err(io_err)
    }
    async fn set_aside_count(&self) -> io::Result<usize> {
        self.mailbox
            .attachment_texts_set_aside()
            .await
            .map_err(io_err)
    }
    async fn raw(&self, user: &str, mid: &str) -> Option<Vec<u8>> {
        let file = self.mailbox.copy_file_of(user, mid).await.ok()??;
        crate::message_util::read_message_raw(&self.maildir_root, user, &file).await
    }
    async fn state(&self, mid: &str) -> io::Result<Option<AttachmentTextState>> {
        self.mailbox
            .attachment_text_state(mid)
            .await
            .map_err(io_err)
    }
    async fn share(&self, user: &str, mid: &str) -> io::Result<()> {
        self.mailbox
            .share_attachment_text(user, mid)
            .await
            .map_err(io_err)
    }
    async fn texts(&self, mid: &str) -> io::Result<Vec<String>> {
        self.mailbox.attachment_texts(mid).await.map_err(io_err)
    }
    async fn store(
        &self,
        mid: &str,
        texts: &[(usize, String)],
        state: &AttachmentTextState,
    ) -> io::Result<()> {
        let kept: Vec<(usize, String)> = texts
            .iter()
            .map(|(n, t)| (*n, cap_text(t.trim(), TEXT_CAP).to_string()))
            .filter(|(_, t)| !t.is_empty())
            .collect();
        self.mailbox
            .store_attachment_texts(mid, &kept, state)
            .await
            .map_err(io_err)
    }
    async fn store_qr(&self, mid: &str, urls: &[String]) -> io::Result<()> {
        self.mailbox
            .store_attachment_qr(mid, &qr_lines(urls), None)
            .await
            .map_err(io_err)
    }
    async fn refresh_qr(&self, mid: &str, urls: &[String], version: u32) -> io::Result<()> {
        self.mailbox
            .store_attachment_qr(mid, &qr_lines(urls), Some(version))
            .await
            .map_err(io_err)
    }
    async fn qr_urls(&self, mid: &str) -> io::Result<Vec<String>> {
        let lines = self
            .mailbox
            .attachment_qr_lines(mid)
            .await
            .map_err(io_err)?;
        Ok(lines.lines().map(str::to_string).collect())
    }
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        kevy_client::Connection::connect(self.net_url.as_ref()?).ok()
    }
}

fn qr_lines(urls: &[String]) -> String {
    kept_qr_urls(urls).join("\n")
}
