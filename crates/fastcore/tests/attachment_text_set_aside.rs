//! A queued attachment the process died reading, every time it tried, is
//! given up on instead of being read again at every restart.
//!
//! The deaths are the attempt counts a dead process leaves in the store:
//! each attempt is counted before the message is read, and only an attempt
//! that ends takes the count away.

use std::sync::Arc;

use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::KevyMailboxStore;

const ATTEMPTS: &[u8] = b"mailrs:attattempts";
const SET_ASIDE: &[u8] = b"mailrs:attsetaside";

#[tokio::test]
async fn an_item_the_worker_died_on_three_times_is_set_aside() {
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let state = Arc::new(FastcoreState::new(store));
    let mb = &state.mailbox;
    mb.queue_attachment_text("bob@x.com", "killer").unwrap();
    mb.queue_attachment_text("bob@x.com", "unlucky").unwrap();
    // three dead attempts at one, two at the other
    mb.store_ref()
        .hincrby(ATTEMPTS, b"bob@x.com\tkiller", 3)
        .unwrap();
    mb.store_ref()
        .hincrby(ATTEMPTS, b"bob@x.com\tunlucky", 2)
        .unwrap();

    let round = mailrs_fastcore::attachment_text::run_once(&state).await;
    assert_eq!(round.taken, 2);

    assert!(
        mb.store_ref()
            .sismember(SET_ASIDE, b"bob@x.com\tkiller")
            .unwrap(),
        "the item it died on three times was not set aside"
    );
    let state_row = mb
        .attachment_text_state("killer")
        .unwrap()
        .expect("stored as tried");
    assert_eq!(state_row.failed, 1);
    // the other was read (it has no copy here) and is not set aside
    assert!(
        !mb.store_ref()
            .sismember(SET_ASIDE, b"bob@x.com\tunlucky")
            .unwrap()
    );
    for mid in ["killer", "unlucky"] {
        assert!(
            !mb.attachment_text_queued("bob@x.com", mid).unwrap(),
            "{mid}"
        );
    }
    assert!(
        mb.store_ref().hgetall(ATTEMPTS).unwrap().is_empty(),
        "attempt counts outlived their items"
    );
    assert_eq!(mb.store_ref().scard(SET_ASIDE).unwrap(), 1);
}
