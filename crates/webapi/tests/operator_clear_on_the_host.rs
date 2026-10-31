//! The operator clears passwords on the host, through the core, and it does
//! what a super administrator's clearing in the web does: sessions end, a
//! mailed recovery link dies, signed-in protocol connections are told to
//! close, the password no longer signs in, and the audit names the
//! operator. The core answers it only with the core secret, the web never
//! serves it, and the production compose publishes the core only on the
//! host's loopback.

mod live;

use reqwest::Method;
use serde_json::json;
use tower::ServiceExt;

const KEVY: &str = "mem://webapi-operator-clear-on-the-host";
const ROOT: &str = "root@example.test";
const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_operator_clears_as_the_web_does() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ROOT, ALICE] {
        live.add_account(who, PW).await;
    }
    live.make_super(ROOT);
    let alice = live.login(ALICE, PW).await;
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    use mailrs_core_sidestate::keys::credentials::{pwreset_by_addr_key, pwreset_key};
    kv.set(pwreset_by_addr_key(ALICE).as_bytes(), b"old-link")
        .unwrap();
    kv.hset(
        pwreset_key("old-link").as_bytes(),
        &[(b"address" as &[u8], ALICE.as_bytes())],
    )
    .unwrap();

    let core = |path: &str| format!("{}{path}", live.core_url);
    let body = json!({ "addresses": [ALICE, "nobody@example.test"] });
    let preview: serde_json::Value = live
        .http
        .post(core("/v1/admin/password-clear:preview"))
        .json(&body)
        .send()
        .await
        .expect("preview")
        .json()
        .await
        .expect("preview body");
    let rows = preview["items"].as_array().expect("items");
    assert_eq!(rows[0]["address"], ALICE, "{preview}");
    assert_eq!(rows[0]["exists"], true);
    assert!(rows[0]["sessions"].as_u64().unwrap_or(0) >= 1, "{preview}");
    assert_eq!(rows[1]["exists"], false);

    let heard = {
        use mailrs_core_sidestate::keys::signout::CHANNEL;
        let mut sub =
            kevy_client::Subscriber::connect_channels(KEVY, &[CHANNEL]).expect("subscribe");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((_, m)) = sub.recv_message() {
                let _ = tx.send(m);
            }
        });
        rx
    };
    let done: serde_json::Value = live
        .http
        .post(core("/v1/admin/password-clear"))
        .json(&body)
        .send()
        .await
        .expect("clear")
        .json()
        .await
        .expect("clear body");
    assert_eq!(done["items"][0]["result"], "cleared", "{done}");
    assert!(done["items"][0]["sessions_revoked"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(done["items"][1]["result"], "unknown");
    let heard = heard
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("no sign-out was published");
    assert_eq!(heard, ALICE.as_bytes());

    let (status, _) = live.call(Method::GET, "/api/auth/me", &alice, None).await;
    assert_eq!(status, 401, "alice's session outlived the clearing");
    let res = live
        .http
        .post(format!("{}/api/auth/login", live.base))
        .json(&json!({ "address": ALICE, "password": PW }))
        .send()
        .await
        .expect("login");
    assert_eq!(res.status(), 401, "the old password still signs in");
    assert!(
        kv.exists(&[
            pwreset_by_addr_key(ALICE).as_bytes(),
            pwreset_key("old-link").as_bytes()
        ])
        .unwrap()
            == 0,
        "the recovery link outlived the clearing"
    );
    let acct = live
        .core
        .get_account_with_hash(ALICE)
        .await
        .expect("account");
    assert_eq!(acct.password_hash.as_deref(), Some(""));
    assert_eq!(
        acct.public.password_cleared_by.as_deref(),
        Some("operator:devops")
    );

    let root = live.login(ROOT, PW).await;
    let (status, log) = live
        .call(
            Method::GET,
            "/api/admin/audit-log?action=account.password_clear",
            &root,
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(
        log["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|r| r["actor"] == "operator:devops" && r["target"] == ALICE),
        "no audit row: {log}"
    );

    // the web has no such route: it is the core's, behind the core secret
    for path in ["/v1/admin/password-clear", "/api/v1/admin/password-clear"] {
        let (status, _) = live
            .call(Method::POST, path, &root, Some(body.clone()))
            .await;
        assert_eq!(status, 404, "the web serves {path}");
    }
}

/// Without the core secret the core refuses both routes.
#[tokio::test]
async fn the_core_refuses_them_without_its_secret() {
    let store = std::sync::Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    );
    let state = std::sync::Arc::new(mailrs_fastcore::FastcoreState::new(
        mailrs_mailbox_kevy::KevyMailboxStore::new(store),
    ));
    let app = mailrs_fastcore::build_router_with_secret(state, "s3cret".into());
    for path in [
        "/v1/admin/password-clear",
        "/v1/admin/password-clear:preview",
    ] {
        for (bearer, want) in [(None, 401), (Some("wrong"), 401)] {
            let mut req =
                axum::http::Request::post(path).header("content-type", "application/json");
            if let Some(b) = bearer {
                req = req.header("authorization", format!("Bearer {b}"));
            }
            let res = app
                .clone()
                .oneshot(
                    req.body(axum::body::Body::from(
                        r#"{"addresses":["a@example.test"]}"#,
                    ))
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), want, "{path} with {bearer:?}");
        }
    }
}

/// Production publishes the core API (port 3301 in its container) on the
/// host's loopback and nowhere else. The same container's IMAP and POP3
/// ports are public on purpose.
#[test]
fn the_core_is_published_on_the_hosts_loopback_only() {
    let mut core_ports = Vec::new();
    for file in ["docker-compose.prod.yml", "docker-compose.pg-core.yml"] {
        let compose = std::fs::read_to_string(format!(
            "{}/../../deploy/{file}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("compose");
        for line in compose.lines().map(str::trim) {
            if line.starts_with('-') && line.trim_end_matches('"').ends_with(":3301") {
                core_ports.push(format!("{file}: {line}"));
            }
        }
    }
    assert!(
        !core_ports.is_empty(),
        "the core's port mapping was not found"
    );
    for p in &core_ports {
        assert!(p.contains("\"127.0.0.1:"), "published beyond the host: {p}");
    }
}
