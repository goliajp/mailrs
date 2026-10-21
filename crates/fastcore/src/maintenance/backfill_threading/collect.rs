//! Reading one account's messages, and repairing the thread rows they
//! disagree with.

use std::collections::HashMap;

use mailrs_core_api::method::message::MessageWire;

use super::Totals;
use crate::FastcoreState;

/// (message_id, in_reply_to, internal_date, tid, blob_ref, subject)
pub(super) type Msg = (String, String, i64, String, String, String);

pub(super) struct UserMessages {
    pub(super) msgs: Vec<Msg>,
    senders_by_tid: HashMap<String, Vec<String>>,
    // last INBOUND message per thread — display time/subject must
    // track the other side, not the user's own replies (2026-07-18).
    last_inbound_by_tid: HashMap<String, (i64, String)>,
}

pub(super) fn user_messages(state: &FastcoreState, user: &str, t: &mut Totals) -> UserMessages {
    // collect every (message_id, in_reply_to, internal_date, tid, blob_ref)
    //
    // Enumerated from the declared `threaduser` rows, not from
    // `user_threads_by_activity`. That zset is legacy — it is in
    // `all_user_thread_zsets`, which `drop-legacy-zsets` deletes — so
    // once it had been dropped this loop saw almost nothing and the
    // backfill became a no-op that still answered 200. Measured on prod
    // 2026-07-30: the zset yielded 9 messages while the table held
    // 30,562 thread-user rows, so every References edge and every merge
    // this exists to perform had silently stopped happening.
    //
    // A keyspace scan, which is what the census does for the same rows.
    // Acceptable here and nowhere near a request path: this is a
    // one-shot admin sweep over the whole mailbox by definition.
    let store = state.mailbox.store_ref();
    let prefix = format!("mailrs:threaduser:{user}:");
    let tids: Vec<Vec<u8>> = store
        .keys(Some(format!("{prefix}*").as_bytes()), None)
        .into_iter()
        .filter_map(|k| {
            let k = String::from_utf8(k).ok()?;
            k.strip_prefix(&prefix).map(|tid| tid.as_bytes().to_vec())
        })
        .collect();
    let tids: Vec<(Vec<u8>, f64)> = tids.into_iter().map(|t| (t, 0.0)).collect();
    t.threads_enumerated += tids.len() as u64;
    let mut found = UserMessages {
        msgs: Vec::new(),
        senders_by_tid: HashMap::new(),
        last_inbound_by_tid: HashMap::new(),
    };
    for (tid_b, _) in &tids {
        let Ok(tid) = std::str::from_utf8(tid_b) else {
            continue;
        };
        t.distinct_tids.insert(tid.to_string());
        for blob in state
            .mailbox
            .thread_messages_for_maintenance(tid)
            .unwrap_or_default()
        {
            if let Ok(w) = serde_json::from_slice::<MessageWire>(&blob) {
                record_message(state, user, tid, w, &mut found, t);
            }
        }
    }
    found
}

fn record_message(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    w: MessageWire,
    found: &mut UserMessages,
    t: &mut Totals,
) {
    let ua = w.user_address.as_str();
    if !ua.is_empty() {
        t.messages_with_user_address += 1;
        if !ua.eq_ignore_ascii_case(user) {
            t.foreign_user_address += 1;
        }
    }
    let list = found.senders_by_tid.entry(tid.to_string()).or_default();
    let sender = w.sender.trim().to_string();
    if !sender.is_empty() && !list.iter().any(|s| s.eq_ignore_ascii_case(&sender)) {
        list.push(sender);
    }
    if !mailrs_mailbox_kevy::senders_csv_contains_user(&w.sender, user) {
        let entry = found
            .last_inbound_by_tid
            .entry(tid.to_string())
            .or_insert((w.internal_date, w.subject.clone()));
        if w.internal_date > entry.0 {
            *entry = (w.internal_date, w.subject.clone());
        }
    }
    // This user's own filename, not the shared blob's —
    // which names whichever owner wrote last and, for 74
    // messages on production, a file in another mailbox.
    // Reading References through it is why
    // `file_unreadable` sat at 109.
    let blob_ref = state
        .mailbox
        .user_message_facts(user, &w.message_id)
        .ok()
        .flatten()
        .map(|f| f.blob_ref)
        .unwrap_or(w.blob_ref);
    found.msgs.push((
        w.message_id,
        w.in_reply_to,
        w.internal_date,
        tid.to_string(),
        blob_ref,
        w.subject,
    ));
}

/// Repair participant unions clobbered by the pre-fix overwrite
/// (a user's own reply used to erase every other participant).
pub(super) fn repair_thread_rows(state: &FastcoreState, user: &str, found: &UserMessages) {
    let mut senders_repaired = 0u64;
    let mut dates_repaired = 0u64;
    for (tid, list) in &found.senders_by_tid {
        let union = list.join(",");
        if let Ok(Some(mut row)) = state.mailbox.get_thread(tid) {
            let mut dirty = false;
            if row.senders_csv != union && !union.is_empty() {
                row.senders_csv = union;
                dirty = true;
                senders_repaired += 1;
            }
            // own replies used to advance latest_date past the last
            // inbound message — pull the row back to inbound time.
            if let Some((date, subject)) = found.last_inbound_by_tid.get(tid)
                && row.latest_date != *date
            {
                row.latest_date = *date;
                if !subject.is_empty() {
                    row.subject = subject.clone();
                }
                dirty = true;
                dates_repaired += 1;
            }
            if dirty && state.mailbox.upsert_thread(user, &row).is_err() {
                tracing::warn!(%user, %tid, "backfill: upsert_thread repair failed");
            }
        }
    }
    if senders_repaired > 0 || dates_repaired > 0 {
        tracing::info!(
            %user,
            senders_repaired,
            dates_repaired,
            "backfill: thread rows repaired"
        );
    }
}
