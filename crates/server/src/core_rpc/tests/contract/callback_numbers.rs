//! Mail with phone numbers in its text and no QR code, on both cores.
//!
//! Numbers in an attachment's text reach the history through the
//! attachment-text worker (`attachment_text`). For mail with no attachment
//! and no QR code, the two cores must leave the same history (the reading
//! pane's callback hint reads it) and judge a report of it alike.

use mailrs_core_sidestate::families::callback_numbers::{self, Given};

use super::harness::{self, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

/// Each core mints its own ids and times.
const MINTED: &[&str] = &["report_id", "at", "decided_at"];

const SENDER: &str = "desk@hotel.example";

/// A letter from the desk, received here at `received` (RFC 5322), whose
/// text gives `number`.
fn letter(message_id: &str, received: &str, number: &str) -> Vec<u8> {
    format!(
        "Received: from mx.hotel.example (192.0.2.7:25)\r\n\tby {} with ESMTP\r\n\
         \tfor <{ALICE}>; {received}\r\n\
         From: Front Desk <{SENDER}>\r\nTo: Alice <{ALICE}>\r\n\
         Subject: Your booking {message_id}\r\nMessage-ID: <{message_id}>\r\n\
         Date: {received}\r\n\r\n\
         To change your booking, call us on {number} (24 hours).\r\n",
        harness::AUTHSERV
    )
    .into_bytes()
}

fn given(url: &str) -> Vec<Given> {
    let mut conn = kevy_client::Connection::connect(url).expect("kevy");
    callback_numbers::given(&mut conn, ALICE, SENDER).expect("history")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn numbers_in_the_text_are_kept_and_judged_alike_on_both_cores() {
    const FC: &str = "mem://callback-numbers-fastcore";
    const PG: &str = "mem://callback-numbers-pg-core";
    let c = harness::start_on(FC, PG).await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    let spools = Spools::new(&c);
    let to_alice = envelope(None);
    for (mid, received, number) in [
        (
            "b1@hotel.example",
            "Mon, 2 Nov 2026 10:00:00 +0000",
            "03-1234-5678",
        ),
        (
            "b2@hotel.example",
            "Tue, 3 Nov 2026 10:00:00 +0000",
            "03 1234 5678",
        ),
        (
            "b3@hotel.example",
            "Wed, 4 Nov 2026 10:00:00 +0000",
            "+81 3 9876 5432",
        ),
    ] {
        spools
            .arrive(&c, &to_alice, &letter(mid, received, number))
            .await;
    }

    let fastcore = given(FC);
    assert_eq!(
        fastcore
            .iter()
            .map(|g| (g.number.as_str(), g.first_message_id.as_str()))
            .collect::<Vec<_>>(),
        [
            ("0312345678", "b1@hotel.example"),
            ("81398765432", "b3@hotel.example"),
        ],
        "the history was not written as the test expects"
    );
    assert_eq!(fastcore, given(PG), "the cores kept different histories");

    // a report of the letter with a new number reads no QR code on either
    d.both(
        "report_message of a letter with a phone number and no QR code",
        c.fastcore
            .report_message(ALICE, "b3@hotel.example", false)
            .await,
        c.pg_core
            .report_message(ALICE, "b3@hotel.example", false)
            .await,
        MINTED,
    );
    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
