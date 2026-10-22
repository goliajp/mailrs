//! Inserting a delivered message.

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
        // Idempotency: if this (mailbox, maildir_id) is already indexed,
        // return the existing uid without reserving a new one. The
        // receiver-decouple notification + reconcile design can ask to
        // index the same delivered maildir file more than once (a re-fired
        // notification, or reconcile racing the live path); reserving a
        // fresh uid each time would create duplicate rows / UID gaps. The
        // messages_mailbox_maildir_uniq index (migrate-047) is the DB-level
        // backstop for any double-index that races past this check.
        if let Some((existing_uid,)) = sqlx::query_as::<_, (i32,)>(
            "SELECT m.uid FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND mb.name = $2 AND m.maildir_id = $3",
        )
        .bind(user)
        .bind(mailbox_name)
        .bind(maildir_id)
        .fetch_optional(&self.pool)
        .await?
        {
            return Ok(existing_uid as u32);
        }

        let mut conn = self.pool.acquire().await?;
        let r = super::uids::reserve(&mut conn, user, mailbox_name, 1).await?;
        let (mailbox_id, uid, new_modseq) = (r.mailbox_id, r.first_uid, r.base_modseq + 1);

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
        .execute(&mut *conn)
        .await?;

        Ok(uid as u32)
    }

    /// Index an already-delivered message: extract headers, resolve
    /// the thread, insert the metadata row, and apply `flags`.
    ///
    /// This is the index-only half of [`Self::append_message`]. The
    /// maildir/object write is the caller's responsibility (it has
    /// already happened, yielding `msg_id`) — keeping the storage
    /// write out of the mailbox stone lets the server route it
    /// through the swappable `MessageStore` seam. Returns the UID.
    pub async fn index_delivered(
        &self,
        user: &str,
        mailbox_name: &str,
        msg_id: &str,
        data: &[u8],
        flags: u32,
        now: i64,
    ) -> Result<u32, String> {
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
            let resolved =
                threading::resolve_thread_id(&message_id, &in_reply_to, |_| parent_tid.clone());
            // Gmail subject rule: joining a KNOWN parent thread requires
            // the normalized subjects to agree — a topic-changing reply
            // starts its own thread. Orphan replies (parent unknown)
            // keep the legacy in_reply_to root unchanged.
            self.apply_subject_gate(user, &message_id, &subject, parent_tid, resolved)
                .await
        } else {
            String::new()
        };

        let uid = self
            .index_message(
                user,
                mailbox_name,
                msg_id,
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

        Ok(uid)
    }
}
