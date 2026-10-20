//! `/api/events` sends each signed-in user their own new-mail events and
//! nobody else's. It used to forward every user's events to every socket
//! and leave the browser to drop the ones that were not its own.

mod live;

use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

const KEVY: &str = "mem://webapi-events-reach-their-user";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(live: &live::Live, token: &str) -> Socket {
    let url = format!(
        "{}/api/events?token={token}",
        live.base.replacen("http", "ws", 1)
    );
    tokio_tungstenite::connect_async(url).await.expect("ws").0
}

/// An event on webapi's bus, as the feed consumer puts it there from what
/// fastcore's `publish_new_mail` wrote. The in-process kevy these tests
/// share has no change feed, so the test feeds the bus itself; what is
/// under test is the fan-out from the bus to each socket.
fn publish(live: &live::Live, user: &str, subject: &str) {
    let body = serde_json::json!({
        "type": "NewMessage", "user": user, "thread_id": "t", "sender": "a@b.c",
        "subject": subject, "snippet": "",
    })
    .to_string();
    let bus = live
        .state
        .event_bus
        .get()
        .expect("the first socket starts the bus");
    bus.send(body).expect("subscribers");
}

async fn subjects(ws: &mut Socket, wait: std::time::Duration) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(Some(Ok(msg))) = tokio::time::timeout(wait, ws.next()).await {
        if let Message::Text(t) = msg {
            let v: serde_json::Value = serde_json::from_str(&t).expect("json");
            out.push(v["subject"].as_str().unwrap_or("").to_string());
        }
    }
    out
}

// The first socket starts webapi's feed consumers, blocking loops that end
// only with the process; a runtime dropped normally, as `#[tokio::test]`
// does and as a panic unwinding through `block_on` does, waits for them
// and never returns.
#[test]
fn each_user_hears_of_their_own_mail_only() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = rt.block_on(rt.spawn(hears_own_mail_only()));
    rt.shutdown_background();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn hears_own_mail_only() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice_token = live.login(ALICE, "pw").await;
    let bob_token = live.login(BOB, "pw").await;
    let mut alice = connect(&live, &alice_token).await;
    let mut bob = connect(&live, &bob_token).await;
    // both sockets subscribe as their upgrade completes
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    publish(&live, BOB, "for-bob");
    publish(&live, "BOB@example.test", "for-bob-capitalised");
    publish(&live, ALICE, "for-alice");

    let wait = std::time::Duration::from_secs(3);
    let mut to_bob = subjects(&mut bob, wait).await;
    let mut to_alice = subjects(&mut alice, wait).await;
    to_bob.sort();
    to_alice.sort();
    assert_eq!(to_bob, ["for-bob", "for-bob-capitalised"]);
    assert_eq!(to_alice, ["for-alice"]);
}
