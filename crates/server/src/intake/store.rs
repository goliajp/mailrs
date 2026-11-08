//! The writes a delivered message makes to the SQL store: its row in the
//! right mailbox, the preview, category and sender verdict beside it, the
//! invitation, the search text, and the fraud verdict with the hold it
//! implies.

use mailrs_intake::arrival::Arrival;
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
        let mb = &self.mailbox;
        mb.ensure_default_mailboxes(addr).await?;
        mb.create_mailbox(addr, mailbox).await?;
        let maildir_id = self.file_into(addr, mailbox, blob_ref)?;
        let input = InsertMessage {
            user: addr,
            mailbox_name: mailbox,
            blob_ref: &maildir_id,
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
        let from_addr = mailrs_core_sidestate::families::correspondent::first_address(&a.from)
            .to_ascii_lowercase();
        let facts = ArrivalFacts {
            snippet: &a.preview,
            category: a.category,
            sender_trust: &a.sender_trust,
            attachments: Some(a.parsed.shown_attachment_count() as u32),
            section: Some(a.facts),
            from_addr: &from_addr,
        };
        let inserted = mb.record_arrival(input, facts).await?;
        // mail from somebody else brings the conversation out of the
        // Trash, or the purge would delete it unread with the rest
        if !a.is_own {
            mb.untrash_thread(addr, &a.thread_id).await?;
        }
        if let Some(found) = &a.invite {
            let payload: serde_json::Value = serde_json::from_str(&found.payload_json)?;
            mb.update_invite_payload(addr, mailbox, inserted.uid, &payload, &found.method)
                .await?;
            mailrs_intake::invite::file_event(
                mailrs_intake::Core::net_conn(self),
                addr,
                found,
                &a.message_id,
            );
        }
        self.index_contents(inserted.id, body, importance).await?;
        let held = self
            .hold_if_fraud(addr, &a.thread_id, &a.message_id, fraud_verdict)
            .await?;
        let mine = mailrs_intake::why::recipient_placement(fraud_verdict, &a.placement);
        if !mine.is_empty() {
            mb.record_message_placement(addr, &a.message_id, &serde_json::to_value(&mine)?)
                .await?;
        }
        Ok(held)
    }

    /// Move the file at `blob_ref` into the maildir folder of `mailbox`, and
    /// return its blob ref there. The drain writes to INBOX or the sieve
    /// folder; a message's body is in its mailbox's folder.
    fn file_into(&self, addr: &str, mailbox: &str, blob_ref: &str) -> std::io::Result<String> {
        let (from_folder, file) = blob_ref.rsplit_once('/').unwrap_or(("", blob_ref));
        let base = crate::body::user_root(&self.dir.maildir_root, addr)?;
        let want = crate::body::folder(&base, mailbox);
        let at = want.join("new").join(file);
        let from = base.join(from_folder).join("new").join(file);
        if at.exists() {
            // filed by an earlier attempt on the same spool file, and the
            // drain wrote it again where it delivers: one copy, not two
            if from != at && from.exists() {
                std::fs::remove_file(&from)?;
            }
        } else {
            for leaf in ["cur", "new", "tmp"] {
                std::fs::create_dir_all(want.join(leaf))?;
            }
            std::fs::rename(from, at)?;
        }
        Ok(crate::body::blob_ref(mailbox, file))
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
        // cleaned in the parse worker; one it fails on is indexed like a
        // message without HTML
        let cleaned = html_body.as_deref().and_then(|html| {
            mailrs_parse_worker::clean_html(html, mailrs_parse_worker::Limit::Inbound).read()
        });
        let (clean_text, has_tracking) = match cleaned {
            Some(c) => (Some(c.clean_text), c.facts.has_tracking_pixel),
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

    /// Store the receiver's verdict, and hold the conversation when it says
    /// to. What is read is left as it is: held mail is out of the badge and
    /// is not pushed, and a release returns it as the reader left it.
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
        if self
            .mailbox
            .hold_thread(addr, thread_id, "ingest", &verdict.rules_version)
            .await?
        {
            self.audit_hold(addr, thread_id, &verdict);
        }
        tracing::info!(
            %addr, %thread_id, score = verdict.score, rules = %verdict.rules_version,
            "held: suspected fraud"
        );
        Ok(true)
    }

    /// The audit line for a conversation that was not held and now is, as
    /// fastcore writes it.
    fn audit_hold(&self, addr: &str, thread_id: &str, v: &mailrs_inbound::FraudVerdict) {
        let Some(mut conn) = self
            .dir
            .net_url
            .as_deref()
            .and_then(|u| kevy_client::Connection::connect(u).ok())
        else {
            tracing::warn!(%thread_id, "no network kevy: the hold audit line was not written");
            return;
        };
        let rules: Vec<&str> = v
            .layers
            .iter()
            .flat_map(|l| l.reasons.iter().map(|r| r.code.as_str()))
            .collect();
        let detail = format!(
            "user={addr} source=ingest rules_version={} rules={}",
            v.rules_version,
            rules.join(",")
        );
        if let Err(e) = mailrs_core_sidestate::families::audit::append(
            &mut conn,
            "system",
            "quarantine.hold",
            thread_id,
            &detail,
            chrono::Utc::now().timestamp(),
        ) {
            tracing::warn!(error = %e, %thread_id, "the hold audit line was not written");
        }
    }
}
