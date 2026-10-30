//! The operations a stolen session could use to stay in or to carry mail
//! away ask for the password again, typed within the last ten minutes on
//! that session. Agent keys, which have no password, are refused them. The
//! confirmation goes through the login guard and ends a session that keeps
//! getting it wrong.
//!
//! One test function: the binary shares one in-process store and one guard,
//! and the steps below depend on each other's failure counts.

mod live;

use mailrs_auth_guard::alerts::Kind;
use mailrs_core_sidestate::keys::session::{confirm_failures_key, confirmed_key, session_ref};
use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-recent-confirmation";
const ADMIN: &str = "admin@example.test";
const VICTIM: &str = "victim@example.test";
const TOTP_USER: &str = "totp@example.test";
const PW: &str = "pw";
const OTHER: &str = "x%40example.test";

/// Every operation that needs a recent confirmation, with a body that gets
/// past nothing else first: the confirmation is the first thing checked.
/// Ids that name nothing, so running them confirmed changes nothing the
/// later steps rely on (group 1 is the one that makes the admin an admin).
fn guarded() -> Vec<(Method, String, Option<Value>)> {
    let q = format!("?user={OTHER}");
    let empty = Some(json!({}));
    vec![
        (
            Method::POST,
            "/api/auth/recovery-email".into(),
            Some(json!({ "recovery_email": "r@example.test" })),
        ),
        (Method::POST, "/api/auth/totp/setup".into(), empty.clone()),
        (
            Method::POST,
            "/api/auth/totp/disable".into(),
            Some(json!({ "code": "000000" })),
        ),
        (
            Method::POST,
            "/api/agent/keys".into(),
            Some(json!({ "name": "k", "scopes": ["mail:read"] })),
        ),
        (
            Method::POST,
            "/api/agent/webhooks".into(),
            Some(json!({ "url": "https://hooks.example/a", "event_type": "new_mail" })),
        ),
        (Method::GET, "/api/mail/export".into(), None),
        (Method::GET, "/api/admin/audit/accounts".into(), None),
        (
            Method::GET,
            format!("/api/admin/audit/conversations{q}"),
            None,
        ),
        (
            Method::GET,
            format!("/api/admin/audit/conversations/t{q}"),
            None,
        ),
        (
            Method::GET,
            format!("/api/admin/audit/conversations/t/messages{q}"),
            None,
        ),
        (
            Method::GET,
            format!("/api/admin/audit/messages/1/raw{q}"),
            None,
        ),
        (Method::GET, "/api/admin/audit-log/export".into(), None),
        (Method::GET, "/api/admin/export".into(), None),
        (Method::POST, "/api/admin/accounts".into(), empty.clone()),
        (
            Method::PUT,
            format!("/api/admin/accounts/{OTHER}"),
            empty.clone(),
        ),
        (Method::DELETE, format!("/api/admin/accounts/{OTHER}"), None),
        (
            Method::POST,
            format!("/api/admin/accounts/{OTHER}/password"),
            empty.clone(),
        ),
        (
            Method::PUT,
            format!("/api/admin/accounts/{OTHER}/forwards"),
            empty.clone(),
        ),
        (
            Method::PUT,
            format!("/api/admin/accounts/{OTHER}/send-as"),
            empty.clone(),
        ),
        (
            Method::PUT,
            format!("/api/admin/accounts/{OTHER}/overrides"),
            empty.clone(),
        ),
        (
            Method::POST,
            format!("/api/admin/accounts/{OTHER}/sieve"),
            empty.clone(),
        ),
        (
            Method::DELETE,
            format!("/api/admin/accounts/{OTHER}/sieve"),
            None,
        ),
        (
            Method::POST,
            "/api/admin/webhook-subscriptions".into(),
            empty.clone(),
        ),
        (Method::POST, "/api/admin/aliases".into(), empty.clone()),
        (Method::DELETE, "/api/admin/aliases/999".into(), None),
        (
            Method::POST,
            "/api/admin/email-groups".into(),
            empty.clone(),
        ),
        (Method::DELETE, "/api/admin/email-groups/999".into(), None),
        (
            Method::POST,
            "/api/admin/email-groups/999/members".into(),
            empty.clone(),
        ),
        (
            Method::DELETE,
            "/api/admin/email-groups/999/members/a%40example.test".into(),
            None,
        ),
        (Method::POST, "/api/admin/groups".into(), empty.clone()),
        (Method::DELETE, "/api/admin/groups/999".into(), None),
        (
            Method::PUT,
            "/api/admin/groups/999/permissions".into(),
            empty.clone(),
        ),
        (
            Method::POST,
            "/api/admin/groups/999/permissions".into(),
            empty.clone(),
        ),
        (
            Method::POST,
            "/api/admin/groups/999/members".into(),
            empty.clone(),
        ),
        (
            Method::DELETE,
            "/api/admin/groups/999/members/a%40example.test".into(),
            None,
        ),
    ]
}

/// The MCP tools behind the same operations.
const GUARDED_TOOLS: [(&str, &str); 4] = [
    (
        "create_email_group",
        r#"{"address":"g@example.test","name":"G","members":[]}"#,
    ),
    (
        "add_alias",
        r#"{"source_address":"a@example.test","target_address":"b@example.test"}"#,
    ),
    (
        "set_account_password",
        r#"{"address":"x@example.test","password":"a long new passphrase"}"#,
    ),
    (
        "audit_list_conversations",
        r#"{"target_user":"x@example.test"}"#,
    ),
];

fn error_of(body: &Value) -> &str {
    body["error"].as_str().unwrap_or("")
}

fn kevy() -> kevy_client::Connection {
    kevy_client::Connection::connect(KEVY).expect("kevy")
}

fn ttl_of_mark(token: &str) -> i64 {
    kevy()
        .ttl_ms(confirmed_key(&session_ref(token)).as_bytes())
        .expect("ttl")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recent_confirmation_guards_what_a_stolen_session_could_abuse() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ADMIN, VICTIM, TOTP_USER] {
        live.add_account(who, PW).await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    // the audit reads are for a super administrator
    live.make_super(ADMIN);
    let call = |token: String, method: Method, path: String, body: Option<Value>| {
        let live = &live;
        async move { live.call(method, &path, &token, body).await }
    };

    // 1. without a confirmation every listed operation is refused, saying why
    let admin = live.login(ADMIN, PW).await;
    for (method, path, body) in guarded() {
        let (status, got) = call(admin.clone(), method.clone(), path.clone(), body).await;
        assert_eq!(
            (status, error_of(&got)),
            (403, "confirm_required"),
            "{method} {path}"
        );
    }
    let (_, state) = call(admin.clone(), Method::GET, "/api/auth/confirm".into(), None).await;
    assert_eq!(state, json!({ "confirmed": false, "expires_in": null }));
    let mut mcp = live.mcp(&admin).await;
    for (tool, args) in GUARDED_TOOLS {
        let refused = mcp.tool(tool, serde_json::from_str(args).unwrap()).await;
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.to_string().contains("confirm_required")),
            "MCP {tool}: {refused:?}"
        );
    }

    // 2. confirmed, none of them is refused for that reason
    live.confirm(&admin, PW).await;
    let (_, state) = call(admin.clone(), Method::GET, "/api/auth/confirm".into(), None).await;
    assert_eq!(state["confirmed"], true);
    let left = state["expires_in"].as_i64().expect("seconds");
    assert!((590..=600).contains(&left), "{left}");
    let before = ttl_of_mark(&admin);
    for (method, path, body) in guarded() {
        let (status, got) = call(admin.clone(), method.clone(), path.clone(), body).await;
        assert_ne!(
            error_of(&got),
            "confirm_required",
            "{method} {path}: {status}"
        );
    }
    let mut mcp = live.mcp(&admin).await;
    for (tool, args) in GUARDED_TOOLS {
        let answer = mcp.tool(tool, serde_json::from_str(args).unwrap()).await;
        assert!(
            !format!("{answer:?}").contains("confirm_required"),
            "MCP {tool}: {answer:?}"
        );
    }
    assert!(
        ttl_of_mark(&admin) <= before,
        "using a confirmation extends it"
    );

    // 3. an agent key has no password: refused, with a code that says so
    let (status, created) = call(
        admin.clone(),
        Method::POST,
        "/api/agent/keys".into(),
        Some(json!({ "name": "ops", "scopes": ["admin"] })),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let key = created["secret"].as_str().expect("secret").to_string();
    for (method, path, body) in [
        (Method::GET, "/api/mail/export".to_string(), None),
        (
            Method::POST,
            "/api/agent/webhooks".into(),
            Some(json!({ "url": "https://h.example/", "event_type": "new_mail" })),
        ),
        (Method::GET, "/api/admin/audit/accounts".into(), None),
        (
            Method::POST,
            "/api/auth/confirm".into(),
            Some(json!({ "password": PW })),
        ),
    ] {
        let (status, got) = call(key.clone(), method.clone(), path.clone(), body).await;
        assert_eq!(
            (status, error_of(&got)),
            (403, "agent_key_not_allowed"),
            "{method} {path}"
        );
    }
    let mut by_key = live.mcp(&key).await;
    let refused = by_key
        .tool(
            "create_email_group",
            serde_json::from_str(GUARDED_TOOLS[0].1).unwrap(),
        )
        .await;
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("agent_key_not_allowed")),
        "{refused:?}"
    );

    // 4. ending the session ends its confirmation
    let mark = confirmed_key(&session_ref(&admin));
    assert_eq!(kevy().exists(&[mark.as_bytes()]).unwrap(), 1);
    let (status, _) = call(admin.clone(), Method::POST, "/api/auth/logout".into(), None).await;
    assert_eq!(status, 204);
    assert_eq!(kevy().exists(&[mark.as_bytes()]).unwrap(), 0, "logout");
    let other = live.login(ADMIN, PW).await;
    live.confirm(&other, PW).await;
    let other_mark = confirmed_key(&session_ref(&other));
    let reset = live.login(ADMIN, PW).await;
    live.confirm(&reset, PW).await;
    let (status, _) = call(
        reset.clone(),
        Method::POST,
        "/api/admin/accounts/admin%40example.test/password".into(),
        Some(json!({ "password": "a much longer new passphrase" })),
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(
        kevy().exists(&[other_mark.as_bytes()]).unwrap(),
        0,
        "revocation"
    );

    // 5. a second factor is asked for when enrolled; a missing code is not
    //    a guess, a wrong one is
    let secret = mailrs_webapi::handlers::totp_util::generate_secret();
    kevy()
        .hset(
            format!("totp:{TOTP_USER}").as_bytes(),
            &[
                (b"secret".as_slice(), secret.as_bytes()),
                (b"enabled", b"1"),
            ],
        )
        .unwrap();
    let code = mailrs_webapi::handlers::totp_util::build_totp(&secret, TOTP_USER)
        .unwrap()
        .generate_current()
        .unwrap();
    let totp_session = {
        let (status, body) = live
            .http
            .post(format!("{}/api/auth/login", live.base))
            .json(&json!({ "address": TOTP_USER, "password": PW, "totp_code": code }))
            .send()
            .await
            .map(|r| (r.status().as_u16(), r))
            .unwrap();
        assert_eq!(status, 200);
        body.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let failures = confirm_failures_key(&session_ref(&totp_session));
    let confirm = |token: String, body: Value| {
        call(token, Method::POST, "/api/auth/confirm".into(), Some(body))
    };
    let (status, got) = confirm(totp_session.clone(), json!({ "password": PW })).await;
    assert_eq!((status, error_of(&got)), (401, "totp_required"));
    assert_eq!(
        kevy().exists(&[failures.as_bytes()]).unwrap(),
        0,
        "not counted"
    );
    let (status, got) = confirm(
        totp_session.clone(),
        json!({ "password": PW, "code": "000000" }),
    )
    .await;
    assert_eq!((status, error_of(&got)), (401, "confirm_failed"));
    let (status, _) = confirm(
        totp_session.clone(),
        json!({ "password": PW, "code": code }),
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(
        totp_session,
        Method::POST,
        "/api/auth/totp/setup".into(),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, 409, "setup would switch an enrolled factor off");

    // 6. five wrong passwords end the session, tell the operators without
    //    the password, and count toward the same lockout as signing in
    let victim = live.login(VICTIM, PW).await;
    for remaining in (1..5).rev() {
        let (status, got) = confirm(victim.clone(), json!({ "password": "guess-SECRET" })).await;
        assert_eq!((status, error_of(&got)), (401, "confirm_failed"));
        assert_eq!(got["remaining"], remaining);
    }
    let (status, got) = confirm(victim.clone(), json!({ "password": "guess-SECRET" })).await;
    assert_eq!((status, error_of(&got)), (401, "session_revoked"));
    let (status, _) = call(victim.clone(), Method::GET, "/api/auth/me".into(), None).await;
    assert_eq!(status, 401, "the session is gone");
    let alerts = mailrs_webapi::login_guard::GUARD
        .take_alerts(100)
        .await
        .unwrap();
    let alert = alerts
        .iter()
        .find(|a| a.kind == Kind::SessionEndedAfterFailedConfirmations)
        .expect("an alert for the operators");
    assert_eq!(alert.subject, VICTIM);
    assert!(
        alert.detail.contains(&session_ref(&victim)[..8]),
        "{}",
        alert.detail
    );
    assert!(!alert.detail.contains("SECRET"), "{}", alert.detail);
    let signing_in = live
        .http
        .post(format!("{}/api/auth/login", live.base))
        .json(&json!({ "address": VICTIM, "password": PW }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        signing_in.status(),
        429,
        "the guesses locked the account here"
    );
}
