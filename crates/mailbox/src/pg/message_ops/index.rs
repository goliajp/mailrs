//! Single-message and batch insertion paths.

use std::collections::HashMap;

use sqlx::QueryBuilder;

use super::IndexRecord;
use crate::pg::PgMailboxStore;
use crate::pg::helpers::extract_header_value;
use crate::threading;

impl PgMailboxStore {
    /// index a new message: assigns UID, inserts metadata, returns UID
    // 11 args mirror the messages-table columns the caller is already
    // computing from a parsed RFC 5322 message — wrapping them in a
    // struct adds a noisy round-trip without simplifying the call site.
    #[allow(clippy::too_many_arguments)]
    pub async fn index_message(
        &self,
        user: &str,
        mailbox_name: &str,
        maildir_id: &str,
        sender: &str,
        recipients: &str,
        subject: &str,
        size: u32,
        now: i64,
        message_id: &str,
        in_reply_to: &str,
        thread_id: &str,
    ) -> Result<u32, sqlx::Error> {
        // Autocommit, no explicit tx. Two non-trivial observations
        // earned by benchmark, both counterintuitive:
        //
        // 1) The UPDATE-RETURNING acquires the mailbox row lock only
        //    for the duration of the statement (~ms). The prior
        //    tx-wrapped SELECT FOR UPDATE → INSERT → UPDATE chain
        //    held the lock across the COMMIT fsync (~17 ms on NVMe),
        //    serialising every concurrent delivery to the same mailbox.
        //    Single-mailbox throughput before round 30: 16.6 msg/s.
        //    After: 134.3 msg/s. (p99: 3.19 s → 69 ms.)
        //
        // 2) Apples-to-apples round 31 measurement, back-to-back on
        //    the same dev cluster, sync_commit=on, identical schema
        //    + workload: wrapping the two writes in an explicit
        //    `BEGIN; UPDATE; INSERT; COMMIT` regresses 1.5× on
        //    100-mailbox fanout (110 → 74 msg/s, p99 +68%) vs
        //    autocommit. PG's group-commit (`commit_delay` /
        //    `commit_siblings`) coalesces concurrent autocommit
        //    COMMITs into shared fsyncs; explicit per-tx COMMITs
        //    each force their own fsync, defeating the batch.
        //
        // 3) R44 tried collapsing the two statements into one
        //    `WITH … INSERT … SELECT FROM reserved` CTE expecting a
        //    -1 RTT win. Measured: fanout=1 flat, fanout=10 +3%,
        //    fanout=100 −6% vs the 2-statement form. PG materialises
        //    CTE results between the UPDATE and INSERT and that
        //    overhead outweighs the saved roundtrip. Two-statement
        //    autocommit kept.
        //
        // The semantic cost of the 2-statement form: a crash between
        // the UPDATE and the INSERT leaves uidnext incremented with
        // no message row at that uid. RFC 9051 §2.3.1.1 explicitly
        // allows UID gaps. The pre-round-30 tx-wrapped code had the
        // equivalent failure mode if a crash happened between
        // maildir.deliver() writing the file and the tx committing.
        let (mailbox_id, uid, new_modseq) = sqlx::query_as::<_, (i64, i32, i64)>(
            "UPDATE mailboxes
             SET uidnext = uidnext + 1,
                 highest_modseq = highest_modseq + 1
             WHERE user_address = $1 AND name = $2
             RETURNING id, uidnext - 1 AS uid, highest_modseq AS new_modseq",
        )
        .bind(user)
        .bind(mailbox_name)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            "INSERT INTO messages (mailbox_id, uid, maildir_id, sender, recipients, subject,
                                   size, date_epoch, internal_date, message_id,
                                   in_reply_to, thread_id, modseq)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(mailbox_id)
        .bind(uid)
        .bind(maildir_id)
        .bind(sender)
        .bind(recipients)
        .bind(subject)
        .bind(size as i32)
        .bind(now)
        .bind(now)
        .bind(message_id)
        .bind(in_reply_to)
        .bind(thread_id)
        .bind(new_modseq)
        .execute(&self.pool)
        .await?;

        Ok(uid as u32)
    }

    /// Batch-index up to N delivered messages in a single explicit
    /// PG transaction. Designed for a DeliveryExecutor-style caller
    /// that has already written the maildir files (and computed all
    /// metadata) for the batch and now needs PG rows in one go.
    ///
    /// Wire shape:
    ///   * One `UPDATE mailboxes … RETURNING base_uid, base_modseq`
    ///     per unique `(user, mailbox_name)` in the batch, to
    ///     reserve a contiguous range of uids in that mailbox.
    ///   * One multi-row `INSERT INTO messages VALUES (...), (...)`
    ///     for the entire batch.
    ///   * One COMMIT.
    ///
    /// Net cost: K + 2 statements + 1 fsync for N deliveries spread
    /// across K mailboxes. Compared to per-message `index_message`
    /// (autocommit, 2N statements + group-commit-coalesced fsyncs),
    /// this trades single-message latency for batch throughput —
    /// per-tx fsync amortises across N rows even when the individual
    /// rows go to different mailboxes.
    ///
    /// Returns the assigned uids in input order. The internal modseq
    /// and uidnext bookkeeping mirrors per-message `index_message`
    /// exactly (CONDSTORE semantics preserved).
    ///
    /// Empty input is a successful no-op.
    pub async fn index_messages_batch(
        &self,
        records: &[IndexRecord<'_>],
    ) -> Result<Vec<u32>, sqlx::Error> {
        if records.is_empty() {
            return Ok(Vec::new());
        }

        // Group records by (user, mailbox) so we can reserve uids
        // in one UPDATE per mailbox. Preserve per-record input
        // order via the inner Vec<usize> of indices into `records`.
        let mut by_mailbox: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
        for (i, r) in records.iter().enumerate() {
            by_mailbox
                .entry((r.user, r.mailbox_name))
                .or_default()
                .push(i);
        }

        let mut tx = self.pool.begin().await?;

        // Reserve uids per mailbox; remember (mailbox_id, base_uid,
        // base_modseq) and the per-record offset within the mailbox.
        let mut mailbox_info: HashMap<(&str, &str), (i64, i32, i64)> = HashMap::new();
        let mut per_record_uid: Vec<i32> = vec![0; records.len()];
        let mut per_record_modseq: Vec<i64> = vec![0; records.len()];
        let mut per_record_mb_id: Vec<i64> = vec![0; records.len()];

        for ((user, mailbox_name), indices) in &by_mailbox {
            let n = indices.len() as i32;
            let (mailbox_id, base_uid, base_modseq): (i64, i32, i64) = sqlx::query_as(
                "UPDATE mailboxes
                 SET uidnext = uidnext + $1, highest_modseq = highest_modseq + $1
                 WHERE user_address = $2 AND name = $3
                 RETURNING id, uidnext - $1 AS base_uid, highest_modseq - $1 AS base_modseq",
            )
            .bind(n)
            .bind(*user)
            .bind(*mailbox_name)
            .fetch_one(&mut *tx)
            .await?;
            mailbox_info.insert((*user, *mailbox_name), (mailbox_id, base_uid, base_modseq));

            for (offset, &record_idx) in indices.iter().enumerate() {
                per_record_mb_id[record_idx] = mailbox_id;
                per_record_uid[record_idx] = base_uid + offset as i32;
                per_record_modseq[record_idx] = base_modseq + offset as i64 + 1;
            }
        }

        // One multi-row INSERT for the whole batch.
        let mut qb: QueryBuilder<crate::pg::BackendDb> = QueryBuilder::new(
            "INSERT INTO messages (mailbox_id, uid, maildir_id, sender, recipients, subject, \
             size, date_epoch, internal_date, message_id, in_reply_to, thread_id, modseq) ",
        );
        qb.push_values(records.iter().enumerate(), |mut b, (i, r)| {
            b.push_bind(per_record_mb_id[i])
                .push_bind(per_record_uid[i])
                .push_bind(r.maildir_id)
                .push_bind(r.sender)
                .push_bind(r.recipients)
                .push_bind(r.subject)
                .push_bind(r.size as i32)
                .push_bind(r.now)
                .push_bind(r.now)
                .push_bind(r.message_id)
                .push_bind(r.in_reply_to)
                .push_bind(r.thread_id)
                .push_bind(per_record_modseq[i]);
        });
        qb.build().execute(&mut *tx).await?;

        tx.commit().await?;

        Ok(per_record_uid.into_iter().map(|u| u as u32).collect())
    }

    /// append a message: write to maildir and index it (with threading)
    /// returns (uid, maildir_id)
    pub async fn append_message(
        &self,
        user: &str,
        mailbox_name: &str,
        maildir_root: &str,
        data: &[u8],
        flags: u32,
        now: i64,
    ) -> Result<(u32, String), String> {
        // extract domain from user
        let (local, domain) = user
            .split_once('@')
            .ok_or_else(|| "invalid user address".to_string())?;

        let path = format!("{maildir_root}/{domain}/{local}");
        let md = mailrs_maildir::Maildir::create(&path)
            .map_err(|e| format!("failed to create maildir: {e}"))?;

        let msg_id = md
            .deliver(data)
            .map_err(|e| format!("failed to deliver: {e}"))?;

        let sender = extract_header_value(data, "From");
        let recipients = extract_header_value(data, "To");
        let subject = extract_header_value(data, "Subject");
        let message_id = threading::extract_message_id(data);
        let in_reply_to = threading::extract_in_reply_to(data);

        let thread_id = if !message_id.is_empty() {
            let parent_tid = self
                .find_thread_id_by_message_id(user, &in_reply_to)
                .await
                .ok()
                .flatten();
            threading::resolve_thread_id(&message_id, &in_reply_to, |_| parent_tid.clone())
        } else {
            String::new()
        };

        let uid = self
            .index_message(
                user,
                mailbox_name,
                &msg_id.to_string(),
                &sender,
                &recipients,
                &subject,
                data.len() as u32,
                now,
                &message_id,
                &in_reply_to,
                &thread_id,
            )
            .await
            .map_err(|e| format!("failed to index: {e}"))?;

        // set flags if any
        if flags != 0 {
            let mb = self
                .get_mailbox(user, mailbox_name)
                .await
                .map_err(|e| format!("failed to get mailbox: {e}"))?
                .ok_or("mailbox not found")?;
            let _ = self.update_flags(mb.id, uid, flags).await;
        }

        Ok((uid, msg_id.to_string()))
    }
}
