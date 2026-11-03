//! `GET /v1/users/{user}/messages/index`: every message the user has, in
//! the compact form a JMAP server compares to see what changed.
//!
//! Building it reads every conversation and message row of the user (at
//! 30,000 messages, seconds), so the last one built per user is kept in
//! memory with the change-feed cursor it was built at. A request first
//! reads the feed from there, filtered to the user's keys; when nothing
//! of theirs was written, the kept index is still right and is answered
//! without reading a row. Without a feed (a store opened without one)
//! nothing is kept and every request builds.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use mailrs_core_api::method::message_index::{Entry, MessageIndex, respond};

use crate::FastcoreState;

/// Feed reads before giving up and building: at 4,096 changes a read,
/// a quarter of a million writes by anyone since the last request.
const MAX_READS: usize = 64;

struct Kept {
    at: (u64, u64),
    body: Arc<Vec<u8>>,
}

fn kept() -> &'static Mutex<HashMap<String, Kept>> {
    static KEPT: OnceLock<Mutex<HashMap<String, Kept>>> = OnceLock::new();
    KEPT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Every key family the index is read from.
fn prefixes(user: &str) -> [String; 4] {
    [
        format!("mailrs:threaduser:{user}:"),
        format!("mailrs:usermsg:{user}:"),
        format!("mailrs:usermsgs:{user}:"),
        format!("mailrs:user:{user}:"),
    ]
}

/// Whether a change wrote one of `prefixes`. A verb without a key (a
/// flush) counts as touching everything.
fn touches(argv: &[Vec<u8>], prefixes: &[&[u8]]) -> bool {
    argv.len() < 2
        || argv[1..]
            .iter()
            .any(|a| prefixes.iter().any(|p| a.starts_with(p)))
}

/// The cursor now, when nothing under the user's keys was written since
/// `at`; `None` when something was, or the feed cannot say.
fn unchanged(store: &kevy_embedded::Store, user: &str, mut at: (u64, u64)) -> Option<(u64, u64)> {
    let owned = prefixes(user);
    let refs: Vec<&[u8]> = owned.iter().map(|p| p.as_bytes()).collect();
    for _ in 0..MAX_READS {
        let batch = store.changes_since(at.0, at.1, 4096, &refs).ok()?;
        if batch.changes.iter().any(|c| touches(&c.argv, &refs)) {
            return None;
        }
        if batch.next == at {
            return Some(at);
        }
        at = batch.next;
    }
    None
}

fn build(state: &FastcoreState, user: &str) -> std::io::Result<Vec<u8>> {
    let mb = &state.mailbox;
    let tids = mb.all_thread_ids_for_user(user)?;
    let mut convs = Vec::with_capacity(tids.len());
    let mut facts = Vec::new();
    for tid in &tids {
        let Some(row) = mb.get_thread_for_user(user, tid)? else {
            continue;
        };
        convs.push(crate::routes::message_ops::row_to_wire(row));
        for mid in mb.user_thread_message_ids(user, tid)? {
            if let Some(f) = mb.user_message_facts(user, &mid)? {
                facts.push((tid.clone(), f));
            }
        }
    }
    let convs = crate::routes::labels::with_labels(state, user, convs);
    let entries: Vec<Entry<'_>> = facts
        .iter()
        .map(|(tid, f)| Entry {
            uid: f.uid,
            thread_id: tid,
            flags: f.flags,
            own: f.own,
        })
        .collect();
    Ok(MessageIndex::build(&convs, &entries).encode())
}

/// The user's index as it is now.
fn current(state: &FastcoreState, user: &str) -> std::io::Result<Arc<Vec<u8>>> {
    let store = state.mailbox.store_ref();
    let mut map = kept().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(k) = map.get_mut(user)
        && let Some(at) = unchanged(store, user, k.at)
    {
        k.at = at;
        return Ok(k.body.clone());
    }
    drop(map);
    // the cursor before the reads: a write while building is seen next time
    let at = store.changes_tail().ok();
    let body = Arc::new(build(state, user)?);
    if let Some(at) = at {
        kept().lock().unwrap_or_else(|e| e.into_inner()).insert(
            user.to_string(),
            Kept {
                at,
                body: body.clone(),
            },
        );
    }
    Ok(body)
}

pub(crate) async fn message_index(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    headers: HeaderMap,
) -> Result<axum::response::Response, StatusCode> {
    let who = user.clone();
    let body = tokio::task::spawn_blocking(move || current(&state, &who))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|e| {
            tracing::warn!(error = %e, %user, "message_index failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(respond(&body, &headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_write_under_the_users_keys_counts() {
        let owned = prefixes("u@x.test");
        let refs: Vec<&[u8]> = owned.iter().map(|p| p.as_bytes()).collect();
        let argv = |parts: &[&str]| {
            parts
                .iter()
                .map(|p| p.as_bytes().to_vec())
                .collect::<Vec<_>>()
        };
        assert!(touches(
            &argv(&["HSET", "mailrs:usermsg:u@x.test:m1", "flags", "1"]),
            &refs
        ));
        assert!(touches(
            &argv(&["DEL", "mailrs:other", "mailrs:threaduser:u@x.test:t"]),
            &refs
        ));
        assert!(!touches(
            &argv(&["HSET", "mailrs:usermsg:v@x.test:m1", "flags", "1"]),
            &refs
        ));
        assert!(!touches(
            &argv(&["HSET", "mailrs:usermsg:u@x.test.evil:m1"]),
            &refs
        ));
        assert!(touches(&argv(&["FLUSHALL"]), &refs));
    }

    const U: &str = "u@x.test";
    const W: &str = "w@x.test";

    fn open(feed_bytes: u64) -> (Arc<kevy_embedded::Store>, FastcoreState) {
        let store = Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default().with_feed(feed_bytes))
                .expect("kevy"),
        );
        let mb = mailrs_mailbox_kevy::KevyMailboxStore::new(store.clone());
        mb.ensure_thread_table();
        (store, FastcoreState::new(mb))
    }

    /// A message for `user`, as delivery writes it.
    fn arrive(state: &FastcoreState, user: &str, tid: &str, mid: &str, uid: u32, flags: u32) {
        let mb = &state.mailbox;
        mb.record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000 + i64::from(uid),
            latest_preview: "hello",
            senders_csv: "Friend <friend@partner.example>",
            subject: "hello",
            thread_id: tid,
            unread: flags == 0,
            user,
        })
        .expect("arrival");
        let payload = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": mid,
            "sender": "Friend <friend@partner.example>", "recipients": user,
            "subject": "hello", "date": 1, "internal_date": 1, "size": 10,
            "flags": flags, "message_id": mid, "in_reply_to": "", "thread_id": tid, "modseq": 1,
        });
        mb.upsert_user_message(
            user,
            tid,
            mid,
            1_786_000_000 + i64::from(uid),
            payload.to_string().as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: mid,
                uid,
                flags,
                modseq: 1,
            },
        )
        .expect("message");
    }

    /// After each way the core changes a user's mail, the answer is what a
    /// fresh build gives; where the index shows the change, it differs
    /// from the answer before.
    #[test]
    fn a_kept_index_follows_every_change_to_the_users_mail() {
        let (_store, state) = open(1 << 20);
        arrive(&state, U, "t1", "m1", 1, 0);
        arrive(&state, U, "t2", "m2", 2, 0);
        let Ok(label) = state.mailbox.create_label(
            U,
            "work",
            mailrs_core_api::method::label::LABEL_COLORS[0],
            0,
        ) else {
            panic!("label not made");
        };
        let mb = state.mailbox.clone();
        type Step<'a> = (&'a str, bool, Box<dyn Fn() + 'a>);
        let steps: Vec<Step<'_>> = vec![
            (
                "arrive",
                true,
                Box::new(|| arrive(&state, U, "t3", "m3", 3, 0)),
            ),
            (
                "flag",
                true,
                Box::new(|| arrive(&state, U, "t1", "m1", 1, 1)),
            ),
            (
                "archive",
                true,
                Box::new(|| drop(mb.set_archived(U, "t1", true))),
            ),
            (
                "unarchive",
                true,
                Box::new(|| drop(mb.set_archived(U, "t1", false))),
            ),
            ("junk", true, Box::new(|| drop(mb.set_junk(U, "t2", true)))),
            (
                "not junk",
                true,
                Box::new(|| drop(mb.set_junk(U, "t2", false))),
            ),
            (
                "trash",
                true,
                Box::new(|| drop(mb.set_trashed(U, "t1", true, 5))),
            ),
            (
                "untrash",
                true,
                Box::new(|| drop(mb.set_trashed(U, "t1", false, 6))),
            ),
            (
                "label",
                true,
                Box::new(|| drop(mb.set_label(U, &label.id, "t2", true))),
            ),
            (
                "unlabel",
                true,
                Box::new(|| drop(mb.set_label(U, &label.id, "t2", false))),
            ),
            (
                "quarantine",
                true,
                Box::new(|| drop(mb.set_quarantined(U, "t3", true))),
            ),
            (
                "release",
                true,
                Box::new(|| drop(mb.set_quarantined(U, "t3", false))),
            ),
            (
                "snooze",
                false,
                Box::new(|| drop(mb.set_snoozed(U, "t2", 2_000_000_000))),
            ),
            (
                "merge",
                false,
                Box::new(|| drop(mb.merge_thread_into(U, "t3", "t2"))),
            ),
            ("delete", true, Box::new(|| drop(mb.delete_thread(U, "t1")))),
        ];
        let mut before = current(&state, U).expect("index");
        for (name, shows, step) in steps {
            step();
            let served = current(&state, U).expect("index");
            let fresh = build(&state, U).expect("fresh");
            assert_eq!(*served, fresh, "after {name}, the kept index is stale");
            if shows {
                assert_ne!(served, before, "{name} does not show in the index");
            }
            before = served;
        }
        state
            .mailbox
            .store_ref()
            .set(b"mailrs:usermsg:v@x.test:m9", b"x")
            .expect("write");
        let again = current(&state, U).expect("index");
        assert!(
            Arc::ptr_eq(&again, &before),
            "another user's write rebuilt the index"
        );
    }

    /// After two conversations are merged, the index puts every message of
    /// both under the one that remains: JMAP's thread ids come from here.
    #[test]
    fn a_merge_moves_the_messages_to_the_conversation_they_joined() {
        const V: &str = "v@x.test";
        let (_store, state) = open(1 << 20);
        arrive(&state, U, "t1", "m1", 1, 0);
        arrive(&state, U, "t2", "m2", 2, 0);
        // the second conversation is also the other account's
        arrive(&state, V, "t2", "m2", 7, 0);
        let thread_in = |who: &str, uid: u32| {
            let idx = MessageIndex::decode(&current(&state, who).expect("index")).expect("decode");
            let m = idx.messages.iter().find(|m| m.uid == uid).expect("message");
            idx.threads[m.thread as usize].thread_id.clone()
        };
        assert_eq!(thread_in(U, 2), "t2");
        state
            .mailbox
            .merge_thread_into(U, "t2", "t1")
            .expect("merge");
        assert_eq!(thread_in(U, 1), "t1");
        assert_eq!(
            thread_in(U, 2),
            "t1",
            "the merged message still names its old conversation"
        );
        // a merge is the merging account's: the other keeps its conversation
        // as it was, whole, in its index and in its own list
        assert_eq!(thread_in(V, 7), "t2");
        assert_eq!(
            state
                .mailbox
                .user_thread_message_ids(V, "t2")
                .expect("list"),
            ["m2"]
        );
    }

    /// A cursor the feed no longer holds means building again, not "no
    /// changes": the user's write may be among what was dropped.
    #[test]
    fn a_cursor_the_feed_dropped_is_built_again() {
        let (store, state) = open(64 * 1024);
        arrive(&state, W, "t1", "m1", 1, 0);
        let before = current(&state, W).expect("index");
        arrive(&state, W, "t1", "m1", 1, 1);
        for n in 0..5_000 {
            store
                .set(format!("mailrs:other:{n}").as_bytes(), &[b'x'; 64])
                .expect("write");
        }
        let at = kept().lock().unwrap().get(W).map(|k| k.at).expect("kept");
        assert!(
            matches!(
                store.changes_since(at.0, at.1, 1, &[]),
                Err(kevy_embedded::FeedError::Resync { .. })
            ),
            "the writes did not push the cursor out of the feed"
        );
        let served = current(&state, W).expect("index");
        assert_ne!(served, before);
        assert_eq!(*served, build(&state, W).expect("fresh"));
    }
}
