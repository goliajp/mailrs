//! Attachment text in this core: the shared worker
//! (`mailrs_intake::attachment_text`) over the embedded store, which keeps
//! the queue and one outcome per message for every recipient.

use std::io;
use std::sync::Arc;

use mailrs_intake::attachment_text::AttachmentTextStore;
pub use mailrs_intake::attachment_text::{
    ATTEMPTS, MAX_PDF_BYTES, Round, VERSION_PDF_ONLY, VERSION_WITH_OCR, WITH_OFFICE, WITH_QR,
    covers, lacks_only_qr, may_carry_readable, reads_anything, version_here, wait_after,
};
use mailrs_mailbox_kevy::AttachmentTextState;

use crate::FastcoreState;

/// This core's [`AttachmentTextStore`].
pub struct KevyAttachmentText(pub Arc<FastcoreState>);

impl AttachmentTextStore for KevyAttachmentText {
    async fn queue(&self, user: &str, mid: &str) -> io::Result<()> {
        self.0.mailbox.queue_attachment_text(user, mid)
    }
    async fn pending(&self, limit: usize) -> io::Result<Vec<(String, String)>> {
        self.0.mailbox.pending_attachment_texts(limit)
    }
    async fn begin(&self, user: &str, mid: &str) -> io::Result<u32> {
        self.0.mailbox.begin_attachment_text(user, mid)
    }
    async fn dequeue(&self, user: &str, mid: &str) -> io::Result<()> {
        self.0.mailbox.dequeue_attachment_text(user, mid)
    }
    async fn set_aside(&self, user: &str, mid: &str) -> io::Result<()> {
        self.0.mailbox.set_aside_attachment_text(user, mid)
    }
    async fn queued(&self, user: &str, mid: &str) -> io::Result<bool> {
        self.0.mailbox.attachment_text_queued(user, mid)
    }
    async fn set_aside_count(&self) -> io::Result<usize> {
        self.0.mailbox.attachment_texts_set_aside()
    }
    async fn raw(&self, user: &str, mid: &str) -> Option<Vec<u8>> {
        let copy = self.0.mailbox.user_message_facts(user, mid).ok()??;
        crate::maildir_scan::read_maildir_file_in(&self.0.maildir_root, user, &copy.blob_ref)
    }
    async fn state(&self, mid: &str) -> io::Result<Option<AttachmentTextState>> {
        self.0.mailbox.attachment_text_state(mid)
    }
    async fn share(&self, _user: &str, _mid: &str) -> io::Result<()> {
        Ok(())
    }
    async fn texts(&self, mid: &str) -> io::Result<Vec<String>> {
        self.0.mailbox.attachment_texts(mid)
    }
    async fn store(
        &self,
        mid: &str,
        texts: &[(usize, String)],
        state: &AttachmentTextState,
    ) -> io::Result<()> {
        self.0.mailbox.store_attachment_texts(mid, texts, state)
    }
    async fn store_qr(&self, mid: &str, urls: &[String]) -> io::Result<()> {
        self.0.mailbox.store_attachment_qr(mid, urls)
    }
    async fn refresh_qr(&self, mid: &str, urls: &[String], version: u32) -> io::Result<()> {
        self.0.mailbox.refresh_attachment_qr(mid, urls, version)
    }
    async fn qr_urls(&self, mid: &str) -> io::Result<Vec<String>> {
        self.0.mailbox.attachment_qr_urls(mid)
    }
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        self.0.net_conn()
    }
}

/// Queue `user`'s copy of the message if any part is readable.
pub(crate) fn queue_if_it_has_readable_parts(
    state: &FastcoreState,
    user: &str,
    message_id: &str,
    parsed: &mailrs_mime::Part<'_>,
) {
    if mailrs_intake::attachment_text::has_readable_parts(parsed)
        && let Err(e) = state.mailbox.queue_attachment_text(user, message_id)
    {
        tracing::warn!(error = %e, %user, %message_id, "queueing attachment text failed");
    }
}

/// One bounded round of the worker over this core's queue.
pub async fn run_once(state: &Arc<FastcoreState>) -> Round {
    mailrs_intake::attachment_text::run_once(&Arc::new(KevyAttachmentText(state.clone()))).await
}

/// The worker, for the life of the process.
pub(crate) fn spawn(state: Arc<FastcoreState>) {
    mailrs_intake::attachment_text::spawn(Arc::new(KevyAttachmentText(state)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_embedded_store_keeps_attachment_text_as_the_contract_says() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = crate::tests::fresh_state_owned();
        state.maildir_root = tmp.path().to_string_lossy().into_owned();
        for user in ["bob@x.com", "carol@x.com"] {
            let dir = tmp
                .path()
                .join("x.com")
                .join(user.split('@').next().unwrap());
            for sub in ["cur", "new", "tmp"] {
                std::fs::create_dir_all(dir.join(sub)).unwrap();
            }
            std::fs::write(dir.join("new").join("f1"), b"Subject: s\r\n\r\nx\r\n").unwrap();
            state
                .mailbox
                .upsert_user_message(
                    user,
                    "t1",
                    "m1",
                    1,
                    b"{}",
                    &mailrs_mailbox_kevy::UserMessageFacts {
                        blob_ref: "f1",
                        uid: 1,
                        flags: 0,
                        modseq: 0,
                    },
                )
                .unwrap();
        }
        let store = KevyAttachmentText(Arc::new(state));
        mailrs_intake::attachment_text::check_attachment_text_store(
            &store,
            "bob@x.com",
            "carol@x.com",
            "m1",
        )
        .await;
    }
}
