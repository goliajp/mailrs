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

use mailrs_core_api::method::message::FLAG_SEEN;
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
    // Once for the sweep: reading it per message would reparse the whole
    // file per message, and this runs over thirty thousand of them.
    let uids = crate::uidlist::load(user);
    // Once for the sweep, like the uidlist: the map is one small file and
    // every message in the mailbox is about to be asked about it.
    let kw = crate::keywords::load(user);
    // And the decisions a bit cannot hold — snooze times and verdicts.
    let log = crate::threadstate::load(user);
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
                //
                // The UID comes from the maildir's own list when it has
                // one: this sweep *is* the rebuild, and a rebuild that
                // invents fresh UIDs is what makes every IMAP client
                // resync.
                let uid =
                    crate::uidlist::uid_for(state, uids.as_ref(), user, &m.message_id, &m.filename);
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
                    // From the file name, not assumed. Maildir states the
                    // read state in the `:2,FLAGS` suffix, and the scan has
                    // already parsed it into `m.seen` — this said `1`, marking
                    // every message it healed as read whatever the file said,
                    // which is the same fact-versus-guess mistake the read-state
                    // work exists to remove.
                    flags: if m.seen { FLAG_SEEN } else { 0 },
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
            // The half that makes the keyword bits worth writing: a
            // decision the maildir carries is put back on the row a
            // rebuilt index serves from. Any message in the thread
            // carrying the bit archives (or pins) the thread, because the
            // bit is per message and the decision was made per thread.
            let archived = ordered
                .iter()
                .any(|m| crate::keywords::file_has(&kw, &m.keywords, crate::keywords::ARCHIVED));
            let pinned = ordered
                .iter()
                .any(|m| crate::keywords::file_has(&kw, &m.keywords, crate::keywords::PINNED));
            if archived {
                let _ = state.mailbox.set_archived(user, root, true);
            }
            if pinned {
                let _ = state.mailbox.set_pinned(user, root, true);
            }
            // The reading half of the thread-state log.
            crate::threadstate::apply_to_row(state, &log, user, root);
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
        // Only the inbound messages **in this scan window**, which is
        // not the same as the thread's inbound messages: the sweep is
        // incremental, so a thread whose only recent file is the user's
        // own reply arrives here with every inbound message out of
        // view.
        //
        // This used to fall back to the plain max when the filter came
        // up empty, on the reasoning that a sent-only thread still
        // needs a date. The fallback cannot tell "this thread is
        // sent-only" from "this window only saw my own message", and
        // for the second case it did exactly what the display rule
        // forbids: it re-dated the conversation to the user's own reply
        // and moved it to the top of Inbox, every sweep, undoing the
        // repair each time (2026-08-12).
        //
        // `None` now means "no inbound evidence here" and the decision
        // moves inside the closure, where the thread's own counters can
        // answer whether it is genuinely sent-only.
        let inbound_max = bucket
            .iter()
            .filter(|m| !mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user))
            .map(|m| m.date)
            .max();
        let window_max = bucket.iter().map(|m| m.date).max().unwrap_or(0);
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
                let count = read_i64(ctx, &thread_key, b"count")?;
                let sent = read_i64(ctx, &thread_key, b"sent_count")?;
                let raised = healed_date(inbound_max, window_max, stored_latest, count, sent);
                if let Some(agg_latest) = raised {
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

/// One numeric field of a hash, or 0 when it is absent or unparseable.
fn read_i64(
    ctx: &mut kevy_embedded::AtomicCtx<'_>,
    key: &str,
    field: &[u8],
) -> Result<i64, kevy_embedded::KevyError> {
    Ok(ctx
        .hget(key.as_bytes(), field)?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0))
}

/// The date this sweep should write, or `None` to leave it alone.
///
/// `inbound_max` is the newest message in **this scan window** that the
/// user did not send — not the thread's newest inbound message, because
/// the sweep is incremental and a window can hold only the user's own
/// reply.
///
/// The rule the whole codebase holds to is that a conversation's row
/// follows its last inbound message. This sweep may only ever raise a
/// date, so its job is to notice an inbound message the row has not
/// caught up with; it must never volunteer the user's own send as that
/// evidence. A thread that genuinely has nothing but the user's own
/// messages is the one exception, and its counters say so.
fn healed_date(
    inbound_max: Option<i64>,
    window_max: i64,
    stored: i64,
    count: i64,
    sent_count: i64,
) -> Option<i64> {
    let sent_only = count > 0 && sent_count >= count;
    let candidate = match inbound_max {
        Some(d) => d,
        None if sent_only => window_max,
        // No inbound message in view and the thread has inbound mail in
        // it somewhere: this window cannot say what the date should be.
        None => return None,
    };
    if candidate > stored {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::healed_date;

    /// The bug, in the shape it was reported: a reply lands, the next
    /// incremental sweep sees only that one file, and the fallback
    /// re-dated the whole conversation to it — every 30 seconds, undoing
    /// each repair.
    #[test]
    fn a_window_holding_only_my_own_reply_leaves_the_date_alone() {
        // 4 messages, 2 of them mine, so the thread is not sent-only.
        assert_eq!(healed_date(None, 1_786_541_659, 1_786_376_013, 4, 2), None);
    }

    #[test]
    fn an_inbound_message_the_row_has_not_caught_up_with_still_raises() {
        assert_eq!(
            healed_date(Some(1_786_541_659), 1_786_541_659, 1_786_376_013, 4, 2),
            Some(1_786_541_659)
        );
    }

    #[test]
    fn a_thread_of_nothing_but_my_own_sends_keeps_using_its_own_date() {
        assert_eq!(healed_date(None, 900, 100, 3, 3), Some(900));
    }

    #[test]
    fn a_date_already_current_is_not_rewritten() {
        assert_eq!(healed_date(Some(900), 900, 900, 4, 1), None);
        assert_eq!(healed_date(Some(100), 900, 900, 4, 1), None);
    }

    /// `count == 0` is a row whose messages were never indexed. That is
    /// missing information, not proof of a sent-only thread.
    #[test]
    fn a_countless_row_is_not_treated_as_sent_only() {
        assert_eq!(healed_date(None, 900, 100, 0, 0), None);
    }
}
