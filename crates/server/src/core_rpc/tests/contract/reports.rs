//! Reporting a scam on both cores: the same template delivered to two
//! people, one of them reports it, and each core holds the reporter's copy,
//! finds the other copy by its fingerprint, lists it for an administrator,
//! and takes it back when the administrator confirms — alike.

use super::harness::{self, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

/// Each core mints its own ids and times.
const MINTED: &[&str] = &["report_id", "at", "decided_at"];

fn scam(name: &str, addr: &str, message_id: &str) -> Vec<u8> {
    format!(
        "From: Parcel Desk <notice@parcel-desk.example>\r\nTo: {name} <{addr}>\r\n\
         Subject: Your parcel is held\r\nMessage-ID: <{message_id}>\r\n\
         Date: Tue, 3 Nov 2026 10:00:00 +0000\r\n\r\n\
         Dear {name}, your parcel could not be delivered because a customs fee is unpaid. \
         Pay the fee at https://parcel-desk.example/pay?to={addr} within 24 hours, otherwise \
         the parcel is returned to the sender and the fee doubles.\r\n"
    )
    .into_bytes()
}

/// The same scam again, replying into the first one's conversation, with
/// our receiver's trace dated an hour from now: it arrived after anything
/// the test did before it.
fn later_copy(name: &str, addr: &str, message_id: &str) -> Vec<u8> {
    let at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc2822();
    let mut raw = format!(
        "Received: from mx.parcel-desk.example (192.0.2.1:25)\r\n\tby {} with ESMTP\r\n\
         \tfor <{addr}>; {at}\r\n\
         In-Reply-To: <s2@parcel-desk.example>\r\nReferences: <s2@parcel-desk.example>\r\n",
        harness::AUTHSERV
    )
    .into_bytes();
    raw.extend(scam(name, addr, message_id));
    raw
}

async fn held_threads(c: &harness::Cores, d: &mut Diffs, user: &str, call: &str) -> Vec<String> {
    let page = d.both(
        call,
        c.fastcore.list_quarantined(user, 50, None).await,
        c.pg_core.list_quarantined(user, 50, None).await,
        &["id", "mailbox_id", "created_at", "modseq", "cursor"],
    );
    page.map(|(_, pg)| pg.items.into_iter().map(|i| i.thread_id).collect())
        .unwrap_or_default()
}

/// In the shared run, where no scam was delivered: a message the reader
/// does not have, a report that does not exist, and an empty waiting list
/// answer alike (404 on both, or the same empty list).
pub(super) async fn run(c: &harness::Cores, d: &mut Diffs) {
    let status = |r: &Result<_, mailrs_core_api::error::CoreApiError>| match r {
        Ok(_) => 200,
        Err(e) => e.status_code(),
    };
    let (a, b) = (
        c.fastcore
            .report_message(ALICE, "nothing@nowhere.test", false)
            .await,
        c.pg_core
            .report_message(ALICE, "nothing@nowhere.test", false)
            .await,
    );
    assert_eq!((status(&a), status(&b)), (404, 404), "{a:?} {b:?}");
    let (a, b) = (
        c.fastcore.report_outcome(ALICE, "rp0000000000000000").await,
        c.pg_core.report_outcome(ALICE, "rp0000000000000000").await,
    );
    assert_eq!((status(&a), status(&b)), (404, 404), "{a:?} {b:?}");
    if let Some((_, pg)) = d.both(
        "pending_reports with none made",
        c.fastcore.pending_reports().await,
        c.pg_core.pending_reports().await,
        MINTED,
    ) {
        assert!(pg.items.is_empty(), "{:?}", pg.items);
    }
    for (what, a, b) in [
        (
            "confirm",
            status(
                &c.fastcore
                    .confirm_report("rp0000000000000000", "a@example.test")
                    .await,
            ),
            status(
                &c.pg_core
                    .confirm_report("rp0000000000000000", "a@example.test")
                    .await,
            ),
        ),
        (
            "dismiss",
            status(
                &c.fastcore
                    .dismiss_report("rp0000000000000000", "a@example.test")
                    .await,
            ),
            status(
                &c.pg_core
                    .dismiss_report("rp0000000000000000", "a@example.test")
                    .await,
            ),
        ),
    ] {
        assert_eq!((a, b), (404, 404), "{what} of a report that does not exist");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_report_finds_and_takes_back_the_other_copies_alike_on_both_cores() {
    let c = harness::start_on(
        "mem://reports-fastcore-side-state",
        "mem://reports-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    let spools = Spools::new(&c);
    let mut to_alice = envelope(None);
    to_alice.forward_paths = vec![ALICE.into()];
    let mut to_bob = envelope(None);
    to_bob.forward_paths = vec![BOB.into()];
    spools
        .arrive(
            &c,
            &to_alice,
            &scam("Alice", ALICE, "s1@parcel-desk.example"),
        )
        .await;
    spools
        .arrive(&c, &to_bob, &scam("Bob", BOB, "s2@parcel-desk.example"))
        .await;

    // one person alone: their copy is held, the other waits
    let reported = d.both(
        "report_message by one reader",
        c.fastcore
            .report_message(ALICE, "s1@parcel-desk.example", false)
            .await,
        c.pg_core
            .report_message(ALICE, "s1@parcel-desk.example", false)
            .await,
        MINTED,
    );
    let (fc_id, pg_id) = match &reported {
        Some((a, b)) => {
            assert_eq!(b.waits, vec!["no_ground".to_string()], "{b:?}");
            assert_eq!(b.inboxes, 1, "{b:?}");
            (a.report_id.clone(), b.report_id.clone())
        }
        None => (String::new(), String::new()),
    };
    let alice_held = held_threads(&c, &mut d, ALICE, "Alice's held list after reporting").await;
    assert_eq!(
        alice_held,
        vec!["s1@parcel-desk.example".to_string()],
        "{}",
        d.0.join("\n\n")
    );
    assert!(
        held_threads(&c, &mut d, BOB, "Bob's held list before a decision")
            .await
            .is_empty()
    );

    let pending = d.both(
        "pending_reports",
        c.fastcore.pending_reports().await,
        c.pg_core.pending_reports().await,
        MINTED,
    );
    if let Some((_, pg)) = &pending {
        assert_eq!(pg.items.len(), 1);
        assert_eq!(pg.items[0].sender_domain, "parcel-desk.example");
        assert_eq!(pg.items[0].samples.len(), 1);
        assert_eq!(pg.items[0].samples[0].user, BOB);
        assert_eq!(pg.items[0].subject, "Your parcel is held");
    }

    // the reporter reads their own outcome; nobody else does
    d.both(
        "report_outcome by the reporter",
        c.fastcore.report_outcome(ALICE, &fc_id).await,
        c.pg_core.report_outcome(ALICE, &pg_id).await,
        MINTED,
    );
    d.both(
        "report_outcome by someone else",
        c.fastcore.report_outcome(BOB, &fc_id).await,
        c.pg_core.report_outcome(BOB, &pg_id).await,
        MINTED,
    );

    let confirmed = d.both(
        "confirm_report",
        c.fastcore
            .confirm_report(&fc_id, "admin@example.test")
            .await,
        c.pg_core.confirm_report(&pg_id, "admin@example.test").await,
        MINTED,
    );
    if let Some((_, pg)) = &confirmed {
        assert_eq!(pg.taken_back, 1, "{pg:?}");
    }
    let bob_held = held_threads(&c, &mut d, BOB, "Bob's held list after confirming").await;
    assert_eq!(bob_held, vec!["s2@parcel-desk.example".to_string()]);
    d.both(
        "pending_reports after confirming",
        c.fastcore.pending_reports().await,
        c.pg_core.pending_reports().await,
        MINTED,
    );

    // Bob puts it back: a later report does not take it again for the copy
    // he already had
    d.both(
        "Bob releases the conversation a report took",
        c.fastcore.release_quarantined(BOB, "s2@parcel-desk.example").await,
        c.pg_core.release_quarantined(BOB, "s2@parcel-desk.example").await,
        &[],
    );
    let again = d.both(
        "an administrator's report after Bob put it back",
        c.fastcore.report_message(ALICE, "s1@parcel-desk.example", true).await,
        c.pg_core.report_message(ALICE, "s1@parcel-desk.example", true).await,
        MINTED,
    );
    if let Some((_, pg)) = &again {
        assert_eq!((pg.inboxes, pg.taken_back), (1, 0), "{pg:?}");
    }
    assert!(
        held_threads(&c, &mut d, BOB, "Bob's held list after he put it back").await.is_empty(),
        "a copy from before Bob's release took his conversation back"
    );

    // a new copy of the template in the same conversation, after the
    // release, may take it
    spools
        .arrive(&c, &to_bob, &later_copy("Bob", BOB, "s2b@parcel-desk.example"))
        .await;
    let later = d.both(
        "an administrator's report after a new copy came",
        c.fastcore.report_message(ALICE, "s1@parcel-desk.example", true).await,
        c.pg_core.report_message(ALICE, "s1@parcel-desk.example", true).await,
        MINTED,
    );
    if let Some((_, pg)) = &later {
        assert_eq!(pg.taken_back, 1, "{pg:?}");
    }
    let bob_held = held_threads(&c, &mut d, BOB, "Bob's held list after the new copy").await;
    assert_eq!(bob_held, vec!["s2@parcel-desk.example".to_string()]);

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
