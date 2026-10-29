//! `/api/events/smtp` is every SMTP session on the server: peer addresses,
//! envelope senders and recipients of everybody's mail. Any signed-in user
//! could subscribe to it, then any administrator; only a super
//! administrator may.

mod live;

use tokio_tungstenite::tungstenite::{Error, client::IntoClientRequest, http::HeaderValue};

const KEVY: &str = "mem://webapi-smtp-trace-is-for-administrators";
const ADMIN: &str = "admin@example.test";
const USER: &str = "user@example.test";
const SUPER: &str = "root@example.test";
const ORIGIN: &str = "https://mail.example.test";

/// The upgrade's HTTP status, with the session in the cookie the way a
/// browser sends it.
async fn upgrade_status(live: &live::Live, token: &str) -> u16 {
    let url = format!("{}/api/events/smtp", live.base.replacen("http", "ws", 1));
    let mut req = url.into_client_request().expect("request");
    req.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("mailrs_session={token}")).expect("cookie"),
    );
    req.headers_mut()
        .insert("origin", HeaderValue::from_static(ORIGIN));
    match tokio_tungstenite::connect_async(req).await {
        Ok((_, resp)) => resp.status().as_u16(),
        Err(Error::Http(resp)) => resp.status().as_u16(),
        Err(e) => panic!("upgrade failed without a status: {e}"),
    }
}

// a subscriber starts blocking feed readers that end only with the
// process; see `events_reach_their_user.rs` for why the runtime is built by
// hand and shut down in the background
#[test]
fn only_an_administrator_may_watch_the_smtp_trace() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = rt.block_on(rt.spawn(only_admins()));
    rt.shutdown_background();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn only_admins() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_WEB_ORIGINS", ORIGIN);
    }
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.add_account(USER, "pw").await;
    live.add_account(SUPER, "pw").await;
    live.grant(ADMIN, "admin.queue").await;
    live.mailbox
        .upsert_permissions(
            SUPER,
            &serde_json::json!({
                "address": SUPER, "permissions": [], "groups": [], "is_super": true, "send_as": [],
            })
            .to_string(),
        )
        .expect("super");
    let admin = live.login(ADMIN, "pw").await;
    let user = live.login(USER, "pw").await;
    let root = live.login(SUPER, "pw").await;

    assert_eq!(upgrade_status(&live, &user).await, 403, "a plain session");
    assert_eq!(
        upgrade_status(&live, &admin).await,
        403,
        "an administrator with one admin.* permission"
    );
    assert_eq!(
        upgrade_status(&live, &root).await,
        101,
        "a super administrator"
    );
    assert_eq!(upgrade_status(&live, "not-a-session").await, 401);
}
