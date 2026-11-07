//! The phone numbers a sender gave a reader before, as the reading pane
//! asks for them: the reader's own history only, earliest first, each
//! naming the message that first gave it.

mod live;

use mailrs_core_sidestate::families::callback_numbers;
use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-callback-numbers-are-the-readers-own";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reader_gets_the_numbers_a_sender_gave_them_and_no_one_else() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    let mut conn = kevy_client::Connection::connect(KEVY).expect("kevy");
    callback_numbers::record_numbers(
        &mut conn,
        ALICE,
        "Desk <Desk@Hotel.example>",
        &["03-1234-5678".to_string()],
        1_790_000_000,
        "<first@hotel.example>",
    )
    .expect("record");
    callback_numbers::record_numbers(
        &mut conn,
        ALICE,
        "desk@hotel.example",
        &["+971 4 123 4567".to_string(), "03 1234 5678".to_string()],
        1_791_000_000,
        "<second@hotel.example>",
    )
    .expect("record");

    let (status, got) = live
        .call(
            Method::GET,
            "/api/senders/DESK%40hotel.example/callback-numbers",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(
        got,
        json!({
            "sender": "desk@hotel.example",
            "numbers": [
                {
                    "number": "0312345678",
                    "first_seen": 1_790_000_000,
                    "first_message_id": "<first@hotel.example>"
                },
                {
                    "number": "97141234567",
                    "first_seen": 1_791_000_000,
                    "first_message_id": "<second@hotel.example>"
                }
            ]
        })
    );

    // the same sender, asked by another reader: nothing of Alice's
    let (status, got) = live
        .call(
            Method::GET,
            "/api/senders/desk%40hotel.example/callback-numbers",
            &bob,
            None,
        )
        .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["numbers"], json!([]));

    // not an address: refused rather than read as an empty history
    let (status, _) = live
        .call(
            Method::GET,
            "/api/senders/not-an-address/callback-numbers",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 400);
}
