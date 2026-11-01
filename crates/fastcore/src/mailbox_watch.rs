//! Tells the web clients when a user's mailbox changed, whoever changed it.
//!
//! The live stream carried new mail and nothing else, so a thread read on
//! a phone, over IMAP or in another tab left every open web client showing
//! it unread, with the badge counting it, until a full reload. Every such
//! change rewrites the user's membership rows (`mailrs:threaduser:<user>:*`),
//! so this reads the embedded store's change feed for that prefix rather
//! than asking each writer to remember to announce itself.
//!
//! Once a second at most per user: the users seen in a tick get one frame
//! each. A quiet tick reads an empty feed slice and writes nothing.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use kevy_embedded::{FeedError, Store};
use mailrs_mailbox_kevy::keys::THREAD_USER_PREFIX;
use tokio::time::MissedTickBehavior;

use crate::FastcoreState;

const TICK: Duration = Duration::from_secs(1);

/// Changes read per call; a tick reads until the feed is drained.
const BATCH: usize = 4096;

pub(crate) fn spawn(state: Arc<FastcoreState>) {
    tokio::spawn(run(state));
}

async fn run(state: Arc<FastcoreState>) {
    let mut at = match state.mailbox.store_ref().changes_tail() {
        Ok(at) => at,
        Err(e) => {
            tracing::warn!(error = ?e, "mailbox watch: no change feed, web clients get no read-state updates");
            return;
        }
    };
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let st = state.clone();
        let read =
            tokio::task::spawn_blocking(move || changed_users(st.mailbox.store_ref(), at)).await;
        let users = match read {
            Ok(Ok((users, next))) => {
                at = next;
                users
            }
            Ok(Err(FeedError::Resync { generation, tail })) => {
                // the changes behind the gap are lost; open clients catch
                // up when they next refetch
                tracing::info!(
                    generation,
                    tail,
                    "mailbox watch: feed resync, resuming at the tail"
                );
                at = (generation, tail);
                continue;
            }
            Ok(Err(e)) => {
                tracing::warn!(error = ?e, "mailbox watch: feed read failed");
                continue;
            }
            Err(e) => {
                tracing::warn!(error = %e, "mailbox watch: feed read task failed");
                continue;
            }
        };
        if users.is_empty() {
            continue;
        }
        let st = state.clone();
        tokio::task::spawn_blocking(move || {
            let Some(mut conn) = st.net_conn() else {
                return;
            };
            for user in &users {
                if let Err(e) =
                    mailrs_core_sidestate::families::live_events::mailbox_changed(&mut conn, user)
                {
                    tracing::warn!(error = %e, "mailbox watch: publishing failed");
                    return;
                }
            }
        });
    }
}

/// The users whose membership rows changed after `at`, and the cursor
/// past what was read.
pub(crate) fn changed_users(
    store: &Store,
    mut at: (u64, u64),
) -> Result<(BTreeSet<String>, (u64, u64)), FeedError> {
    let mut users = BTreeSet::new();
    loop {
        let batch = store.changes_since(at.0, at.1, BATCH, &[THREAD_USER_PREFIX])?;
        // the prefix filter lets multi-key verbs through, so each key is asked
        for change in &batch.changes {
            for arg in change.argv.iter().skip(1) {
                if let Some(user) = user_of(arg) {
                    users.insert(user.to_string());
                }
            }
        }
        let drained = batch.next == at || batch.changes.len() < BATCH;
        at = batch.next;
        if drained {
            return Ok((users, at));
        }
    }
}

/// The user a membership-row key belongs to: `mailrs:threaduser:<user>:<tid>`.
/// An address holds no `:`, a thread id may.
fn user_of(key: &[u8]) -> Option<&str> {
    let rest = key.strip_prefix(THREAD_USER_PREFIX)?;
    let end = rest.iter().position(|b| *b == b':')?;
    std::str::from_utf8(&rest[..end])
        .ok()
        .filter(|u| !u.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_is_read_up_to_the_first_colon_after_the_prefix() {
        assert_eq!(
            user_of(b"mailrs:threaduser:bob@x.test:t1"),
            Some("bob@x.test")
        );
        assert_eq!(
            user_of(b"mailrs:threaduser:bob@x.test:<a:b@c>"),
            Some("bob@x.test")
        );
        assert_eq!(user_of(b"mailrs:threaduser:bob@x.test"), None);
        assert_eq!(user_of(b"mailrs:thread:t1"), None);
    }

    fn mailbox() -> mailrs_mailbox_kevy::KevyMailboxStore {
        let store = Store::open(kevy_embedded::Config::default().with_feed(1 << 20)).unwrap();
        let mb = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(store));
        mb.ensure_thread_table();
        mb
    }

    fn arrive(mb: &mailrs_mailbox_kevy::KevyMailboxStore, user: &str, tid: &str) {
        mb.record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            thread_id: tid,
            user,
            subject: "s",
            senders_csv: "bob@y.test",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }

    #[test]
    fn a_thread_read_is_seen_for_its_user_and_no_one_else() {
        let mb = mailbox();
        arrive(&mb, "me@x.test", "t1");
        arrive(&mb, "other@x.test", "t2");
        let at = mb.store_ref().changes_tail().unwrap();

        // nothing written since: nothing to say
        let (quiet, at) = changed_users(mb.store_ref(), at).unwrap();
        assert!(quiet.is_empty(), "{quiet:?}");

        assert!(mb.mark_seen("me@x.test", "t1").unwrap());
        let (users, at) = changed_users(mb.store_ref(), at).unwrap();
        assert_eq!(users.into_iter().collect::<Vec<_>>(), ["me@x.test"]);

        // and once said, it is not said again
        let (again, _) = changed_users(mb.store_ref(), at).unwrap();
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn an_archive_is_seen_too() {
        let mb = mailbox();
        arrive(&mb, "me@x.test", "t1");
        let at = mb.store_ref().changes_tail().unwrap();
        assert!(mb.set_archived("me@x.test", "t1", true).unwrap());
        let (users, _) = changed_users(mb.store_ref(), at).unwrap();
        assert!(users.contains("me@x.test"), "{users:?}");
    }
}
