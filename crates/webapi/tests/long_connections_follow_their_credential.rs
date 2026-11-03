//! An open JMAP WebSocket or `/api/events` socket closes within a minute of
//! what signed it in stopping to work: the session signed out, the
//! password changed, the app password or agent key revoked, the account
//! deleted. A connection whose credential still holds stays open. And a
//! token in the URL never signs anything in.

mod live;

use std::time::Duration;

use base64::Engine as _;
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::json;
use tokio_tungstenite::tungstenite::{
    Error as WsError, Message, client::IntoClientRequest, http::HeaderValue,
};

const KEVY: &str = "mem://webapi-long-connections-follow-their-credential";
const ORIGIN: &str = "https://mail.example.test";
const ADMIN: &str = "admin@example.test";
const A: &str = "a@example.test";
const B: &str = "b@example.test";
const C: &str = "c@example.test";
const D: &str = "d@example.test";
const E: &str = "e@example.test";
const KEEP: &str = "keep@example.test";

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn open(
    live: &live::Live,
    path: &str,
    headers: &[(&'static str, String)],
) -> Result<Socket, u16> {
    let url = format!("{}{path}", live.base.replacen("http", "ws", 1));
    let mut req = url.into_client_request().expect("request");
    for (k, v) in headers {
        req.headers_mut()
            .insert(*k, HeaderValue::from_str(v).expect("header"));
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((s, _)) => Ok(s),
        Err(WsError::Http(r)) => Err(r.status().as_u16()),
        Err(e) => panic!("upgrade failed without a status: {e}"),
    }
}

fn jmap_headers(auth: String) -> Vec<(&'static str, String)> {
    vec![
        ("authorization", auth),
        ("sec-websocket-protocol", "jmap".into()),
        ("x-forwarded-proto", "https".into()),
    ]
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn basic(user: &str, password: &str) -> String {
    let raw = format!("{user}:{password}");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw)
    )
}

/// Whether the socket is closed by the server within `secs`.
async fn closes_within(mut s: Socket, secs: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, s.next()).await {
            Err(_) => return false,
            Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => return true,
            Ok(Some(Ok(_))) => continue,
        }
    }
}

// accepted upgrades start feed readers that end only with the process
#[test]
fn a_long_connection_ends_with_its_credential() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = rt.block_on(rt.spawn(async {
        tokio::time::timeout(Duration::from_secs(240), run())
            .await
            .expect("did not finish within 4 minutes")
    }));
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
        std::env::set_var("MAILRS_HOSTNAME", "mail.example.test");
    }
    let live = live::start(KEVY).await;
    for who in [ADMIN, A, B, C, D, E, KEEP] {
        live.add_account(who, "pw").await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;

    // a token in the URL signs nothing in, over HTTP or the socket
    let a = live.login(A, "pw").await;
    let r = live
        .http
        .post(format!("{}/jmap/api/?access_token={a}", live.base))
        .json(&json!({ "using": ["urn:ietf:params:jmap:core"], "methodCalls": [] }))
        .send()
        .await
        .expect("post");
    assert_eq!(r.status(), 401);
    for q in ["access_token", "token"] {
        let refused = open(
            &live,
            &format!("/jmap/ws/?{q}={a}"),
            &[("sec-websocket-protocol", "jmap".into())],
        )
        .await;
        assert_eq!(refused.err(), Some(401), "?{q}=");
    }

    // A: a session signed out — its JMAP socket and its events socket
    let a_jmap = open(&live, "/jmap/ws/", &jmap_headers(bearer(&a)))
        .await
        .expect("a jmap");
    let a_events = open(
        &live,
        "/api/events",
        &[
            ("cookie", format!("mailrs_session={a}")),
            ("origin", ORIGIN.into()),
        ],
    )
    .await
    .expect("a events");

    // B: the password changes — a session and an account-password login
    let b = live.login(B, "pw").await;
    let b_session = open(&live, "/jmap/ws/", &jmap_headers(bearer(&b)))
        .await
        .expect("b session");
    let b_password = open(&live, "/jmap/ws/", &jmap_headers(basic(B, "pw")))
        .await
        .expect("b basic");

    // C: an app password revoked
    let c = live.login(C, "pw").await;
    live.confirm(&c, "pw").await;
    let (status, made) = live
        .call(
            Method::POST,
            "/api/auth/app-passwords",
            &c,
            Some(json!({ "name": "aerc" })),
        )
        .await;
    assert_eq!(status, 201, "{made}");
    let c_app = made["password"].as_str().expect("password").to_string();
    let c_app_id = made["id"].as_str().expect("id").to_string();
    let c_socket = open(&live, "/jmap/ws/", &jmap_headers(basic(C, &c_app)))
        .await
        .expect("c app");

    // D: an agent key revoked
    let d = live.login(D, "pw").await;
    live.confirm(&d, "pw").await;
    let (status, made) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &d,
            Some(json!({ "name": "script", "scopes": ["jmap"] })),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let d_key = made["secret"].as_str().expect("secret").to_string();
    let d_key_id = made["id"].clone();
    let d_socket = open(&live, "/jmap/ws/", &jmap_headers(bearer(&d_key)))
        .await
        .expect("d key");

    // E: the account deleted, signed in with its password
    let e_socket = open(&live, "/jmap/ws/", &jmap_headers(basic(E, "pw")))
        .await
        .expect("e basic");

    // KEEP: nothing happens to it; it stays open
    let keep = live.login(KEEP, "pw").await;
    let keep_socket = open(&live, "/jmap/ws/", &jmap_headers(bearer(&keep)))
        .await
        .expect("keep");

    let (status, _) = live.call(Method::POST, "/api/auth/logout", &a, None).await;
    assert_eq!(status, 204);
    let (status, body) = live
        .call(
            Method::POST,
            "/api/auth/change-password",
            &b,
            Some(json!({ "current_password": "pw", "new_password": "a much longer new password 42" })),
        )
        .await;
    assert!(status < 300, "{status} {body}");
    let (status, _) = live
        .call(
            Method::DELETE,
            &format!("/api/auth/app-passwords/{c_app_id}"),
            &c,
            None,
        )
        .await;
    assert!(status < 300, "{status}");
    let key_id = match &d_key_id {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let key_path = format!("/api/agent/keys/{key_id}");
    let (status, _) = live.call(Method::DELETE, &key_path, &d, None).await;
    assert!(status < 300, "{status}");
    let (status, _) = live
        .call(
            Method::DELETE,
            "/api/admin/accounts/e%40example.test",
            &admin,
            None,
        )
        .await;
    assert_eq!(status, 204);

    let closed = futures_util::future::join_all(
        [
            a_jmap, a_events, b_session, b_password, c_socket, d_socket, e_socket,
        ]
        .into_iter()
        .map(|s| closes_within(s, 60)),
    )
    .await;
    let names = [
        "signed out (jmap)",
        "signed out (events)",
        "password changed (session)",
        "password changed (basic)",
        "app password revoked",
        "agent key revoked",
        "account deleted",
    ];
    for (closed, name) in closed.iter().zip(names) {
        assert!(closed, "{name}: still open after 60 s");
    }
    assert!(
        !closes_within(keep_socket, 5).await,
        "an untouched connection was closed"
    );
}
