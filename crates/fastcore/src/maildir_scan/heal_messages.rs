//! Healing what the store is missing about messages already on disk.
//!
//! Two passes: a one-shot uid backfill (guarded by a persistent sentinel
//! so it does not re-scan every tick), and the per-thread diff that writes
//! message wires the store never got — the "spool drain crashed mid-tick"
//! case, where the file is on disk and the API cannot see it.

use mailrs_core_api::method::message::FLAG_SEEN;
use std::sync::Arc;

use super::scan::MailFile;
use crate::FastcoreState;

/// One-shot per boot, per user.
pub(crate) fn backfill_uids_once(state: &Arc<FastcoreState>, user: &str) {
    // UID backfill — one-time per boot per user. Repair any
    // MessageWire that self-heal wrote before we started allocating
    // uids (all showed uid=0, breaking /api/mail/messages/{uid}/…
    // attachment endpoints). Guard on a persistent flag so subsequent
    // ticks don't re-scan the full mailbox. Bump the sentinel key when
    // the migration format changes to force another sweep.
    // v2: bumped after finding deliver_message wrote uid=0 wires for
    // every web-sent mirror copy until 2026-07-03 — one more full sweep
    // repairs the backlog now that the write path allocates correctly.
    let uid_flag_key = format!("mailrs:user:{user}:uid_backfill_v2");
    let need_uid_backfill = state
        .mailbox
        .store_ref()
        .get(uid_flag_key.as_bytes())
        .ok()
        .flatten()
        .is_none();
    if need_uid_backfill {
        // Declared rows; `user_threads_by_activity` is legacy and unwritten,
        // so this healed 168 threads install-wide instead of 30,562.
        let all_tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        let mut uid_healed = 0u32;
        for tid in &all_tids {
            let tid = tid.as_str();
            let msgs = state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default();
            for payload in msgs {
                let Ok(mut wire) = serde_json::from_slice::<
                    mailrs_core_api::method::message::MessageWire,
                >(&payload) else {
                    continue;
                };
                // This user's uid, not the shared blob's — that one belongs
                // to whichever owner wrote last, so testing it would skip
                // the second owner of a thread and leave them without one.
                // The whole per-user row, not just its uid. This used to
                // take only `.uid` and then write `blob_ref` and `flags`
                // back from `wire` — the **shared** blob, which is stripped
                // of exactly those fields by design. So a backfill whose
                // only job is to assign a uid also blanked the file
                // reference and reset the read state.
                //
                // `upsert_user_message` now refuses to erase a known
                // `blob_ref` or `uid` with an unknown one, but it cannot
                // protect `flags`: zero is a real value there, so a zero
                // arriving from the stripped blob is indistinguishable from
                // "mark unread" and would be applied. Hence this reads the
                // row and passes it back unchanged apart from the uid.
                let known = state
                    .mailbox
                    .user_message_facts(user, &wire.message_id)
                    .ok()
                    .flatten();
                let existing = known.as_ref().map(|f| f.uid).unwrap_or(wire.uid);
                if existing != 0 {
                    continue;
                }
                let uid = state
                    .mailbox
                    .allocate_uid(user, &wire.message_id)
                    .unwrap_or(0);
                if uid == 0 {
                    continue;
                }
                wire.uid = uid;
                if let Ok(new_payload) = serde_json::to_vec(&wire) {
                    let _ = state.mailbox.upsert_user_message(
                        user,
                        &wire.thread_id,
                        &wire.message_id,
                        wire.internal_date,
                        &new_payload,
                        &mailrs_mailbox_kevy::UserMessageFacts {
                            blob_ref: known
                                .as_ref()
                                .map(|f| f.blob_ref.as_str())
                                .unwrap_or(&wire.blob_ref),
                            uid: wire.uid,
                            flags: known.as_ref().map(|f| f.flags).unwrap_or(wire.flags),
                            modseq: known.as_ref().map(|f| f.modseq).unwrap_or(wire.modseq),
                        },
                    );
                }
                uid_healed += 1;
            }
        }
        let _ = state.mailbox.store_ref().set(uid_flag_key.as_bytes(), b"1");
        if uid_healed > 0 {
            tracing::info!(%user, uid_healed, "self-heal: uid backfill (one-shot)");
        }
    }
}

/// Write the messages a thread has on disk and not in the store.
///
/// Returns `(healed_threads, healed_msgs, diff_healed_threads,
/// diff_healed_msgs)` — the two pairs count the two branches apart,
/// because "the index was empty" and "the index was missing one" are
/// different faults.
pub(crate) fn heal_missing_messages(
    state: &Arc<FastcoreState>,
    user: &str,
    by_root: &std::collections::HashMap<String, Vec<&MailFile>>,
    files_scanned: usize,
) -> (u32, u32, u32, u32) {
    // Walk threads and heal — two branches, both idempotent:
    // (a) zset empty → populate all bucket messages (original behaviour)
    // (b) zset non-empty but bucket has message-ids not in it → G14.2
    //     diff branch. Catches the "spool_drain crashed / dropped a file
    //     mid-tick" case: the file's on disk but the wire never got
    //     written, so the message is invisible to the API. Diffing by
    //     message-id closes that gap without touching the fast path.
    // Declared rows. The zset this replaced was legacy and unwritten; the
    // 0..999 slice it took is kept as an explicit bound because this runs
    // on a timer, and the count below reports what was actually walked.
    let mut tids = state
        .mailbox
        .all_thread_ids_for_user(user)
        .unwrap_or_default();
    tids.truncate(1000);
    let mut healed_threads = 0u32;
    let mut healed_msgs = 0u32;
    let mut diff_healed_threads = 0u32;
    let mut diff_healed_msgs = 0u32;
    // Once for the sweep — see `uidlist::load`.
    let uids = crate::uidlist::load(user);
    for tid in &tids {
        let tid = tid.as_str();
        let msg_zset = mailrs_mailbox_kevy::keys::thread_messages(tid);
        let existing_count = state
            .mailbox
            .store_ref()
            .zcard(msg_zset.as_bytes())
            .unwrap_or(0);
        let Some(bucket) = by_root.get(tid) else {
            continue;
        };

        // Compute (message_id → &MailFile) index for the bucket up front —
        // used by both branches. Filter out entries with no Message-ID so
        // upsert_message doesn't key on an empty string (which would
        // conflate distinct files into one wire).
        let bucket_by_mid: std::collections::HashMap<&str, &&MailFile> = bucket
            .iter()
            .filter(|m| !m.message_id.is_empty())
            .map(|m| (m.message_id.as_str(), m))
            .collect();
        if bucket_by_mid.is_empty() {
            continue;
        }

        // Determine which of the bucket's messages need writing:
        // empty zset → all of them; non-empty → diff against existing
        // wire payloads' message_id field.
        let missing_mids: Vec<&str> = if existing_count == 0 {
            bucket_by_mid.keys().copied().collect()
        } else {
            let existing_mids: std::collections::HashSet<String> = state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|payload| {
                    serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(
                        &payload,
                    )
                    .ok()
                })
                .map(|w| w.message_id)
                .collect();
            bucket_by_mid
                .keys()
                .copied()
                .filter(|mid| !existing_mids.contains(*mid))
                .collect()
        };
        if missing_mids.is_empty() {
            continue;
        }
        // Sort by date so zadd scores are chronological — matches the
        // spool_drain write path so mixed populate/diff runs produce the
        // same ordering.
        let mut to_write: Vec<&MailFile> = missing_mids
            .into_iter()
            .filter_map(|mid| bucket_by_mid.get(mid).copied().copied())
            .collect();
        to_write.sort_by_key(|m| m.date);
        for m in &to_write {
            // From the maildir's own list when it has one, so a rebuild
            // keeps the UIDs it has already promised to clients; otherwise
            // allocate, which is idempotent — reruns return the
            // previously-issued uid via the uid_by_mid reverse index.
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
                // Read back off the file by the scan, so a healed row
                // carries what the delivered one did.
                invite_method: m
                    .invite
                    .as_ref()
                    .map(|i| i.method.clone())
                    .unwrap_or_default(),
                thread_id: tid.to_string(),
                modseq: 0,
                user_address: user.to_string(),
            };
            if let Some(found) = &m.invite {
                crate::invites::store(state, &m.message_id, found);
            }
            let payload = match serde_json::to_vec(&wire) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let _ = state.mailbox.upsert_user_message(
                user,
                tid,
                &wire.message_id,
                m.date,
                &payload,
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref: &wire.blob_ref,
                    uid: wire.uid,
                    flags: wire.flags,
                    modseq: wire.modseq,
                },
            );
            let _ = state
                .mailbox
                .set_thread_for_message_id(user, &wire.message_id, tid);
            if existing_count == 0 {
                healed_msgs += 1;
            } else {
                diff_healed_msgs += 1;
            }
        }
        if existing_count == 0 {
            healed_threads += 1;
        } else {
            diff_healed_threads += 1;
        }
    }
    if healed_threads > 0 {
        tracing::info!(
            %user, healed_threads, healed_msgs, files_scanned,
            "self-heal (maildir): populated missing messages"
        );
    }
    if diff_healed_threads > 0 {
        // G14.2 diff branch fired — surface it separately so an oncall
        // can distinguish "brand-new thread stitched from scratch" from
        // "existing thread patched with a message the drain missed".
        tracing::info!(
            %user,
            diff_healed_threads,
            diff_healed_msgs,
            files_scanned,
            "self-heal (maildir): diff branch patched missed messages (G14.2)"
        );
    }

    (
        healed_threads,
        healed_msgs,
        diff_healed_threads,
        diff_healed_msgs,
    )
}
