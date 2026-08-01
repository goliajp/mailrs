//! Healing the per-user membership row from what is on disk.
//!
//! Creates the thread aggregate for mail that arrived with no row at all,
//! and repairs a stale `activity` or a `senders_csv` that names the user
//! without the `is_sender` flag derived from it — which is how a thread
//! the user had replied in stayed out of Sent.
//!
//! Every write here is conditional and the closure reports whether
//! anything changed. It used to `zadd` a legacy index and bump its counter
//! unconditionally, so a fully healed mailbox logged `sent_added=255
//! created=0` every 31 seconds forever
//! (`.claude/rules/periodic-work-must-converge.md`).

use std::sync::Arc;

use super::scan::MailFile;
use crate::FastcoreState;

/// Returns `(rows_healed, created)`.
pub(crate) fn heal_membership_rows(
    state: &Arc<FastcoreState>,
    user: &str,
    by_root: &std::collections::HashMap<String, Vec<&MailFile>>,
) -> (u32, u32) {
    // ── Sent-index backfill ──────────────────────────────────────
    //
    // The historical migration derived senders_csv from monolith's
    // sent_count aggregate, which false-negatives on many threads the
    // user actually sent messages to (only 22 of ~200 sent messages
    // were picked up on lihao@golia.jp). Walk every thread bucket and,
    // if any of its maildir files has From: == user, add the thread's
    // fastcore tid to `mailrs:user:<u>:threads:sent` scored by the
    // latest sent message's date. Idempotent: zadd overwrites the
    // score for a tid that's already there.
    let mut rows_healed = 0u32;
    let mut created = 0u32;
    for (root, bucket) in by_root {
        let sent_here: Vec<&&MailFile> = bucket
            .iter()
            .filter(|m| mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user))
            .collect();
        let is_sender_thread = !sent_here.is_empty();
        let thread_key = mailrs_mailbox_kevy::keys::thread(root);
        let exists = state
            .mailbox
            .store_ref()
            .hexists(thread_key.as_bytes(), b"count")
            .unwrap_or(false);
        if !exists {
            // Create a minimal thread aggregate from scratch — inbound
            // OR outbound. Skipping non-sender threads here was the
            // reason fresh Gmail arrivals (files present in maildir but
            // no matching kevy hash) never showed up in the inbox: the
            // "heal missing messages" branch above only touches threads
            // already in the by_activity zset, so a genuinely new
            // arrival had no path in. Create here for every bucket.
            let mut ordered: Vec<&MailFile> = bucket.to_vec();
            ordered.sort_by_key(|m| m.date);
            for m in &ordered {
                let category = "inbox";
                let is_own = mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user);
                let unread = !m.seen && !is_own;
                let arrival = mailrs_mailbox_kevy::MessageArrival {
                    thread_id: root,
                    user,
                    subject: &m.subject,
                    senders_csv: &m.from,
                    latest_date: m.date,
                    latest_preview: "",
                    category,
                    unread,
                    is_own,
                };
                let _ = state.mailbox.record_message_arrival(&arrival);
                // Side sink: contacts autocomplete.
                crate::live_sync::upsert_contacts(user, &m.from);
                // Also write the message blob for enrich_with_body.
                let uid = state.mailbox.allocate_uid(user, &m.message_id).unwrap_or(0);
                let wire = mailrs_core_api::method::message::MessageWire {
                    id: 0,
                    mailbox_id: 0,
                    uid,
                    blob_ref: m.filename.clone(),
                    sender: m.from.clone(),
                    recipients: m.to.clone(),
                    subject: m.subject.clone(),
                    date: m.date,
                    internal_date: m.date,
                    size: m.size,
                    flags: 1,
                    message_id: m.message_id.clone(),
                    in_reply_to: m.in_reply_to.clone(),
                    sender_trust: m.sender_trust.clone(),
                    thread_id: root.clone(),
                    modseq: 0,
                    user_address: user.to_string(),
                };
                if let Ok(payload) = serde_json::to_vec(&wire) {
                    let _ = state.mailbox.upsert_user_message(
                        user,
                        root,
                        &m.message_id,
                        m.date,
                        &payload,
                        &mailrs_mailbox_kevy::UserMessageFacts {
                            blob_ref: &wire.blob_ref,
                            uid: wire.uid,
                            flags: wire.flags,
                            modseq: wire.modseq,
                        },
                    );
                }
                let _ = state
                    .mailbox
                    .set_thread_for_message_id(user, &m.message_id, root);
            }
            created += 1;
        }
        if !is_sender_thread {
            // Inbound-only thread — created (or already existed), but
            // the user isn't a sender, so the Sent axis below does not
            // apply to it.
            continue;
        }
        // Heal the aggregate's own latest_date when the stored one is
        // stale or zero (a hash written back when parse_rfc5322_date
        // fed 0), so the row stops sinking to the bottom of the list.
        //
        // Display-date semantics (2026-07-18): the row follows the last
        // INBOUND message, so the heal must not treat the user's own
        // sent copy as newer truth — that exact write undid the
        // backfill repair every 30 s. Sent-only threads keep the plain
        // max.
        let bucket_max = bucket
            .iter()
            .filter(|m| !mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user))
            .map(|m| m.date)
            .max()
            .unwrap_or_else(|| bucket.iter().map(|m| m.date).max().unwrap_or(0));
        let tu_key = mailrs_mailbox_kevy::keys::thread_user(user, root);
        // Every write below is conditional, and the closure reports
        // whether it actually changed anything. A self-heal that
        // re-does its work every cycle is a busy-wait, not a heal: this
        // loop used to zadd the sent zset and bump `sent_added`
        // unconditionally, so a fully-healed mailbox still logged
        // `sent_added=255 created=0` every 31 s forever (2026-07-19).
        let changed = state
            .mailbox
            .store_ref()
            .atomic(|ctx| {
                let mut changed = false;
                let stored_latest = ctx
                    .hget(thread_key.as_bytes(), b"latest_date")?
                    .and_then(|v| String::from_utf8(v).ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);
                let agg_latest = std::cmp::max(stored_latest, bucket_max);
                if agg_latest > stored_latest {
                    ctx.hset(
                        thread_key.as_bytes(),
                        &[(b"latest_date" as &[u8], agg_latest.to_string().as_bytes())],
                    )?;
                    // The membership row carries the same fact as
                    // `activity`, and every axis sorts on that one — so
                    // healing the hash alone would move the date pill
                    // and leave the order it is supposed to explain.
                    ctx.hset(
                        tu_key.as_bytes(),
                        &[(b"activity" as &[u8], agg_latest.to_string().as_bytes())],
                    )?;
                    changed = true;
                }
                // Merge user into the thread's senders_csv so future
                // upsert_thread invocations (mark_read etc.) don't drop
                // sent-index membership.
                let cur_csv = ctx
                    .hget(thread_key.as_bytes(), b"senders_csv")?
                    .and_then(|v| String::from_utf8(v).ok())
                    .unwrap_or_default();
                if !mailrs_mailbox_kevy::senders_csv_contains_user(&cur_csv, user) {
                    let new_csv = if cur_csv.is_empty() {
                        user.to_string()
                    } else {
                        format!("{cur_csv}, {user}")
                    };
                    ctx.hset(
                        thread_key.as_bytes(),
                        &[(b"senders_csv" as &[u8], new_csv.as_bytes())],
                    )?;
                    // `is_sender` is derived from senders_csv, and the
                    // Sent axis keys on it. Writing the CSV without the
                    // flag is how a thread stayed out of Sent while the
                    // hash said the user had written in it — the defect
                    // the zadd here used to paper over.
                    ctx.hset(tu_key.as_bytes(), &[(b"is_sender" as &[u8], b"1" as &[u8])])?;
                    changed = true;
                }
                Ok(changed)
            })
            .unwrap_or(false);
        if changed {
            rows_healed += 1;
        }
    }
    if rows_healed > 0 || created > 0 {
        tracing::info!(
            %user, rows_healed, created,
            "self-heal (maildir): membership rows"
        );
    }
    (rows_healed, created)
}
