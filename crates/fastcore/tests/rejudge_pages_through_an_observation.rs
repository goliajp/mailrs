//! An observation that names more mail than one page is judged over
//! several, its place kept in fastcore's store: a round cut short goes on
//! where it stopped, and a round with nothing new writes nothing. The
//! rules see the reader's history with the sender, so the BEC rules match
//! on this path. With acting off a hold is only logged.

mod rejudge_support;

use mailrs_reputation::Stance;
use rejudge_support::*;

const KEVY: &str = "mem://fastcore-rejudge-pages";

#[tokio::test]
async fn an_observation_is_judged_page_by_page_and_resumed_where_it_stopped() {
    // one test in its own integration-test process owns these variables
    unsafe {
        std::env::set_var("MAILRS_REJUDGE_PAGE", "2");
        std::env::remove_var("MAILRS_REJUDGE_ACT");
    }
    let mut w = world(KEVY);
    let mids: Vec<String> = (1..=5)
        .map(|n| format!("inv-{n}@mail.vendorbill.com"))
        .collect();
    for (n, mid) in mids.iter().enumerate() {
        add(&w, mid, &letter(mid, n == 0));
    }
    assert_eq!(
        round(&w.state).await.0,
        0,
        "the first round starts from the counter"
    );

    // five letters, pages of two: all five judged in one round
    observe(&mut w.kv, Stance::Deny, "vendorbill.com");
    let (judged, counts) = round(&w.state).await;
    assert_eq!(judged, 5, "{counts}");
    assert_eq!(counts["paged"], 1, "{counts}");
    for mid in &mids {
        let v = w.state.mailbox.ledger_verdict(USER, mid).unwrap().unwrap();
        assert!(
            v.rules.iter().any(|r| r == "bec-pay-to-a-stranger"),
            "{mid}: the reader never heard from this sender: {:?}",
            v.rules
        );
    }
    // acting is off: the program is only logged as a hold
    assert!(
        w.state
            .mailbox
            .ledger_verdict(USER, &mids[0])
            .unwrap()
            .unwrap()
            .holds
    );
    assert!(!held(&w.state, &mids[0]), "shadow moves nothing");
    assert_eq!(counts["acted"], 0);

    // nothing new: a round writes nothing at all
    let before = tail(&w.state);
    assert_eq!(round(&w.state).await.0, 0);
    assert_eq!(tail(&w.state), before, "an idle round wrote to the store");

    // a round stopped after its first page of the next observation: the
    // place it kept is where the next round starts
    observe(&mut w.kv, Stance::Allow, "vendorbill.com");
    let store = w.state.mailbox.store_ref();
    let seq: u64 = String::from_utf8(store.get(b"rejudge:obs_seq").unwrap().unwrap())
        .unwrap()
        .parse()
        .unwrap();
    store
        .set(b"rejudge:pending", format!("{} 2", seq + 1).as_bytes())
        .unwrap();
    let (judged, counts) = round(&w.state).await;
    assert_eq!(
        judged, 3,
        "the two judged before the stop are not judged again: {counts}"
    );
    assert!(
        store.get(b"rejudge:pending").unwrap().is_none(),
        "the place is cleared when done"
    );

    let before = tail(&w.state);
    assert_eq!(round(&w.state).await.0, 0);
    assert_eq!(tail(&w.state), before, "an idle round wrote to the store");
}
