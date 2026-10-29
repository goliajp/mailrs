//! A WebSocket upgrade is a GET, so the CSRF token does not cover it. One
//! the session cookie authenticates must name one of our web origins; a
//! browser always sends `Origin` on an upgrade.

mod live;

use tokio_tungstenite::tungstenite::{Error, client::IntoClientRequest, http::HeaderValue};

const KEVY: &str = "mem://webapi-websocket-needs-our-origin";
const USER: &str = "ws@example.test";
const ORIGIN: &str = "https://mail.example.test";

async fn status(live: &live::Live, token: &str, origin: Option<&str>) -> u16 {
    let url = format!("{}/api/events", live.base.replacen("http", "ws", 1));
    let mut req = url.into_client_request().expect("request");
    req.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("mailrs_session={token}")).expect("cookie"),
    );
    if let Some(o) = origin {
        req.headers_mut()
            .insert("origin", HeaderValue::from_str(o).expect("origin"));
    } else {
        // tungstenite adds no Origin of its own
        req.headers_mut().remove("origin");
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((_, resp)) => resp.status().as_u16(),
        Err(Error::Http(resp)) => resp.status().as_u16(),
        Err(e) => panic!("upgrade failed without a status: {e}"),
    }
}

// an accepted upgrade starts blocking feed readers that end only with the
// process; see `events_reach_their_user.rs`
#[test]
fn a_cookie_upgrade_must_come_from_our_origin() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = rt.block_on(rt.spawn(run()));
    rt.shutdown_background();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn run() {
    // SAFETY: the only test in this binary, before anything reads the env
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_WEB_ORIGINS", ORIGIN);
    }
    let live = live::start(KEVY).await;
    live.add_account(USER, "pw").await;
    let token = live.login(USER, "pw").await;

    assert_eq!(status(&live, &token, None).await, 403, "no origin");
    assert_eq!(
        status(&live, &token, Some("https://evil.example")).await,
        403
    );
    assert_eq!(status(&live, &token, Some(ORIGIN)).await, 101);
}
