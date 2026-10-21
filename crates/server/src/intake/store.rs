//! The writes a delivered message makes to the SQL store: its row in the
//! right mailbox, the preview, category and sender verdict beside it, the
//! invitation, the search text, and the fraud verdict with the hold it
//! implies.

use mailrs_intake::arrival::Arrival;
use mailrs_intake::maildir::maildir_subfolder;
use mailrs_intelligence::importance::ImportanceLevel;
use mailrs_mailbox::pg::ArrivalFacts;
use mailrs_mailbox::{InsertMessage, StoreError};

use super::PgCore;
use super::ingest::importance_columns;

impl PgCore {
    /// Returns whether the conversation was held as fraud.
    #[allow(clippy::too_many_arguments)] // one call site, each argument a different fact
    pub(super) async fn store(
        &self,
        addr: &str,
        mailbox: &str,
        blob_ref: &str,
        body: &[u8],
        a: &Arrival<'_>,
        importance: Option<(ImportanceLevel, f32)>,
        fraud_verdict: Option<&str>,
    ) -> Result<bool, StoreError> {
        let maildir_id = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
        let mb = &self.mailbox;
        mb.ensure_default_mailboxes(addr).await?;
        mb.create_mailbox(addr, mailbox).await?;
        self.file_into(addr, mailbox, blob_ref)?;
        let input = InsertMessage {
            user: addr,
            mailbox_name: mailbox,
            blob_ref: maildir_id,
            sender: &a.from,
            recipients: &a.to,
            subject: &a.subject,
            size: body.len() as u32,
            date: a.date,
            internal_date: a.date,
            message_id: &a.message_id,
            in_reply_to: &a.in_reply_to,
            thread_id: &a.thread_id,
            flags: match a.is_own {
                true => mailrs_mailbox::types::FLAG_SEEN,
                false => 0,
            },
        };
        let facts = ArrivalFacts {
            snippet: &a.preview,
            category: a.category,
            sender_trust: &a.sender_trust,
        };
        let inserted = mb.record_arrival(input, facts).await?;
        if let Some(found) = mailrs_intake::invite::find(body) {
            let payload: serde_json::Value = serde_json::from_str(&found.payload_json)?;
            mb.update_invite_payload(addr, mailbox, inserted.uid, &payload, &found.method)
                .await?;
            mailrs_intake::invite::file_event(mailrs_intake::Core::net_conn(self), addr, &found);
        }
        self.index_contents(inserted.id, body, importance).await?;
        self.hold_if_fraud(addr, &a.thread_id, &a.message_id, fraud_verdict)
            .await
    }

    /// Move the file at `blob_ref` into the maildir folder of `mailbox`. The
    /// drain writes to INBOX or the sieve folder; this store finds a
    /// message's file in its mailbox's folder.
    fn file_into(&self, addr: &str, mailbox: &str, blob_ref: &str) -> std::io::Result<()> {
        let (from_folder, maildir_id) = blob_ref.rsplit_once('/').unwrap_or(("", blob_ref));
        let Some((local, domain)) = addr.split_once('@') else {
            return Ok(());
        };
        let base = std::path::Path::new(&self.dir.maildir_root)
            .join(domain)
            .join(local);
        let want = match mailbox {
            "INBOX" => base.clone(),
            other => base.join(maildir_subfolder(other)),
        };
        let at = want.join("new").join(maildir_id);
        if at.exists() {
            return Ok(());
        }
        for leaf in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(want.join(leaf))?;
        }
        std::fs::rename(base.join(from_folder).join("new").join(maildir_id), at)
    }

    /// The text search and the thread view read, and the importance the
    /// shared scorer gave an inbound message.
    async fn index_contents(
        &self,
        id: i64,
        body: &[u8],
        importance: Option<(ImportanceLevel, f32)>,
    ) -> Result<(), sqlx::Error> {
        use mailrs_clean as clean;
        let (text_body, html_body, _) = crate::message_util::parse_message(body);
        let head = String::from_utf8_lossy(&body[..body.len().min(4096)]);
        let is_bulk = clean::detect_bulk_sender(&head);
        let (clean_text, has_tracking) = match &html_body {
            Some(html) => {
                let cleaned = clean::clean_email_html(html);
                (Some(cleaned.clean_text), cleaned.has_tracking_pixel)
            }
            None => (text_body.clone(), false),
        };
        let new_content = clean_text
            .as_deref()
            .map(|t| clean::split_quoted_content(t).0);
        let (level, score) = importance_columns(importance);
        self.mailbox
            .update_message_content(
                id,
                text_body.as_deref(),
                html_body.as_deref(),
                clean_text.as_deref(),
                new_content.as_deref(),
                is_bulk,
                has_tracking,
                level,
                score,
            )
            .await
    }

    /// Store the receiver's verdict, and hold the conversation, read, when
    /// it says to: a held conversation that is also unread is bold, counts
    /// in the badge and rings the phone, which is what holding removes.
    async fn hold_if_fraud(
        &self,
        addr: &str,
        thread_id: &str,
        message_id: &str,
        fraud_verdict: Option<&str>,
    ) -> Result<bool, StoreError> {
        let Some(json) = fraud_verdict else {
            return Ok(false);
        };
        // written by the receiver, another process: one that does not parse
        // is reported and holds nothing, as in the kevy core
        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, %addr, "the receiver's verdict is not JSON");
                return Ok(false);
            }
        };
        self.mailbox
            .record_fraud_verdict(message_id, &value)
            .await?;
        let verdict: mailrs_inbound::FraudVerdict = match serde_json::from_value(value) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, %addr, "the stored verdict did not parse");
                return Ok(false);
            }
        };
        if !verdict.quarantined {
            return Ok(false);
        }
        self.mailbox.hold_thread(addr, thread_id).await?;
        self.mailbox.mark_thread_read(addr, thread_id, None).await?;
        tracing::info!(
            %addr, %thread_id, score = verdict.score, rules = %verdict.rules_version,
            "held: suspected fraud"
        );
        Ok(true)
    }
}
