//! An account whose password was cleared comes back with a code an issuer
//! hands its owner: the issuer (who need not be an administrator) issues
//! it, the store keeps only its MAC, the sign-in page learns whom to ask,
//! and the owner sets a new password with it and signs in. Wrong codes,
//! weak passwords, a used code and five failures each behave as they
//! must, and the recovery-mail route offers no way around it.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-reset-codes-set-a-new-password";
const ISSUER: &str = "issuer@example.test";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const NEW: &str = "a sound new passphrase";

async fn post(live: &live::Live, path: &str, body: Value) -> (u16, Value) {
    let res = live
        .http
        .post(format!("{}{path}", live.base))
        .json(&body)
        .send()
        .await
        .expect("post");
    let status = res.status().as_u16();
    let text = res.text().await.expect("body");
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn reset(live: &live::Live, code: &str, new_password: &str) -> (u16, Value) {
    post(
        live,
        "/api/auth/password-reset",
        json!({ "address": ALICE, "code": code, "new_password": new_password }),
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_code_from_an_issuer_sets_a_new_password() {
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_RESET_ISSUERS", ISSUER);
        std::env::set_var("MAILRS_RESET_CONTACT_NAME", "Li Hao");
        std::env::set_var("MAILRS_RESET_CONTACT", ISSUER);
    }
    let live = live::start(KEVY).await;
    for who in [ISSUER, ALICE, BOB] {
        live.add_account(who, "pw").await;
    }
    // a recovery link mailed before the clearing, to be found dead after it
    let mut kevy = kevy_client::Connection::connect(KEVY).expect("kevy");
    kevy.hset(
        b"pwreset:old-link",
        &[(b"address" as &[u8], ALICE.as_bytes())],
    )
    .unwrap();
    kevy.set(format!("pwreset_by_addr:{ALICE}").as_bytes(), b"old-link")
        .unwrap();
    live.core
        .clear_account_password(
            ALICE,
            &mailrs_core_api::method::admin::ClearPasswordRequest { by: ISSUER.into() },
        )
        .await
        .expect("clear");
    let (status, _) = post(
        &live,
        "/api/auth/reset-password",
        json!({ "token": "old-link", "new_password": NEW }),
    )
    .await;
    assert_eq!(status, 401, "a recovery link undid the clearing");

    let bob = live.login(BOB, "pw").await;
    live.confirm(&bob, "pw").await;
    let (status, got) = live
        .call(
            Method::POST,
            &format!("/api/reset-codes/{ALICE}"),
            &bob,
            None,
        )
        .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (403, Some("not_an_issuer"))
    );

    let issuer = live.login(ISSUER, "pw").await;
    // the settings page offers reset codes to an issuer, and to nobody else
    let (_, me) = live.call(Method::GET, "/api/auth/me", &issuer, None).await;
    assert_eq!(me["is_reset_issuer"], true);
    let (_, me) = live.call(Method::GET, "/api/auth/me", &bob, None).await;
    assert_eq!(me["is_reset_issuer"], false);
    let issue = |path: String| {
        let (live, issuer) = (&live, issuer.clone());
        async move { live.call(Method::POST, &path, &issuer, None).await }
    };
    let (status, got) = issue(format!("/api/reset-codes/{ALICE}")).await;
    assert_eq!(
        (status, got["error"].as_str()),
        (403, Some("confirm_required"))
    );
    live.confirm(&issuer, "pw").await;
    let (status, got) = issue(format!("/api/reset-codes/{ALICE}")).await;
    assert_eq!(
        (status, got["error"].as_str()),
        (501, Some("reset_codes_not_configured"))
    );
    // SAFETY: as above
    unsafe { std::env::set_var("MAILRS_RESET_CODE_KEY", "11".repeat(32)) };
    let (status, got) = issue(format!("/api/reset-codes/{BOB}")).await;
    assert_eq!(
        (status, got["error"].as_str()),
        (409, Some("password_not_cleared"))
    );

    let (status, first) = issue(format!("/api/reset-codes/{ALICE}")).await;
    assert_eq!(status, 200, "issue: {first}");
    let first = first["code"].as_str().expect("code").to_string();
    let (status, issued) = issue(format!("/api/reset-codes/{ALICE}")).await;
    assert_eq!(status, 200);
    let code = issued["code"].as_str().expect("code").to_string();
    assert_ne!(code, first);
    let row = kevy
        .hgetall(mailrs_core_sidestate::keys::reset::code_key(ALICE).as_bytes())
        .unwrap();
    assert!(!row.is_empty(), "nothing stored");
    assert!(
        !row.iter()
            .any(|v| v.windows(code.len()).any(|w| w == code.as_bytes())),
        "the code is stored in clear"
    );
    let alerts = mailrs_webapi::login_guard::GUARD
        .take_alerts(100)
        .await
        .unwrap();
    let issued_alerts = alerts
        .iter()
        .filter(|a| {
            a.kind == mailrs_auth_guard::alerts::Kind::ResetCodeIssued && a.subject == ALICE
        })
        .count();
    assert_eq!(issued_alerts, 2, "each code is reported");

    // the sign-in page learns whom to ask, whatever password was typed
    let (status, got) = post(
        &live,
        "/api/auth/login",
        json!({ "address": ALICE, "password": "pw" }),
    )
    .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (401, Some("password_cleared"))
    );
    assert_eq!(
        got["contact"],
        json!({ "name": "Li Hao", "address": ISSUER })
    );
    let (status, got) = post(
        &live,
        "/api/auth/login",
        json!({ "address": BOB, "password": "wrong" }),
    )
    .await;
    assert_eq!(
        (status, got),
        (401, Value::Null),
        "an account not cleared is told nothing new"
    );

    // the earlier code stopped working when the second was issued
    assert_eq!(reset(&live, &first, NEW).await.0, 400);
    let (status, got) = reset(&live, "WRONG23456", NEW).await;
    assert_eq!(
        (status, got["error"].as_str()),
        (400, Some("invalid_reset_code"))
    );
    let (status, got) = live
        .call(
            Method::GET,
            &format!("/api/reset-codes/{ALICE}"),
            &issuer,
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        (got["issued"].as_bool(), got["failures"].as_u64()),
        (Some(true), Some(2)),
        "{got}"
    );
    assert_eq!(got["key_current"], true);
    assert!(got.get("code").is_none(), "the status shows the code");

    // a weak password is refused and the code kept; typed with a dash and
    // in lower case, it still works
    let (status, got) = reset(&live, &code, "short").await;
    assert_eq!(status, 400, "{got}");
    assert_ne!(got["error"], "invalid_reset_code");
    let typed = format!("{}-{}", &code[..5], &code[5..]).to_ascii_lowercase();
    assert_eq!(reset(&live, &typed, NEW).await.0, 204);

    let after = live.login(ALICE, NEW).await;
    let (status, _) = live.call(Method::GET, "/api/auth/me", &after, None).await;
    assert_eq!(status, 200);
    let acct = live
        .core
        .get_account_with_hash(ALICE)
        .await
        .expect("account");
    assert!(
        acct.public.password_cleared_at.is_none(),
        "still marked cleared"
    );
    assert_eq!(
        reset(&live, &code, "another sound passphrase").await.0,
        400,
        "a used code worked again"
    );

    let audit: Vec<Value> = kevy
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice(r).ok())
        .collect();
    for action in [
        "reset_code.issue",
        "reset_code.failed",
        "account.password_set_by_code",
    ] {
        assert!(
            audit
                .iter()
                .any(|r| r["action"] == action && r["target"] == ALICE),
            "no {action} in the audit log"
        );
    }
    assert!(
        !audit.iter().any(|r| r.to_string().contains(&code)),
        "a code reached the audit log"
    );

    // the fifth wrong code ends the code: bob, whose four earlier wrong
    // tries are written straight in, stays under the login guard's limit
    live.core
        .clear_account_password(
            BOB,
            &mailrs_core_api::method::admin::ClearPasswordRequest { by: ISSUER.into() },
        )
        .await
        .expect("clear bob");
    let (status, got) = issue(format!("/api/reset-codes/{BOB}")).await;
    assert_eq!(status, 200, "{got}");
    let bob_code = got["code"].as_str().expect("code").to_string();
    kevy.set(
        mailrs_core_sidestate::keys::reset::failures_key(BOB).as_bytes(),
        b"4",
    )
    .unwrap();
    let wrong = json!({ "address": BOB, "code": "WRONG23456", "new_password": NEW });
    assert_eq!(post(&live, "/api/auth/password-reset", wrong).await.0, 400);
    let (_, got) = live
        .call(
            Method::GET,
            &format!("/api/reset-codes/{BOB}"),
            &issuer,
            None,
        )
        .await;
    assert_eq!(
        got["issued"], false,
        "the code outlived five wrong tries: {got}"
    );
    let right = json!({ "address": BOB, "code": bob_code, "new_password": NEW });
    assert_eq!(post(&live, "/api/auth/password-reset", right).await.0, 400);
}
