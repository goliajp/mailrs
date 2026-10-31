//! The server's own observations, recorded by hand: only a super
//! administrator may, every one carries a reason, and a mistake is
//! withdrawn rather than deleted.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-screening-observations";
const ADMIN: &str = "admin@example.test";
const USER: &str = "user@example.test";
const PW: &str = "pw";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observations_are_recorded_by_a_super_administrator_with_a_reason() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, PW).await;
    live.add_account(USER, PW).await;
    live.make_super(ADMIN);
    let admin = live.login(ADMIN, PW).await;
    let user = live.login(USER, PW).await;
    let call = |who: &str, m: Method, p: &str, b: Option<Value>| {
        let (who, live) = (who.to_string(), &live);
        let p = p.to_string();
        async move { live.call(m, &p, &who, b).await }
    };
    let incident = json!({"source": "incident", "kind": "ip", "value": "200.181.82.134", "stance": "deny",
                          "reason": "2026-10-24 incident: brute force from this address"});

    let (status, _) = call(
        &user,
        Method::POST,
        "/api/admin/screening/observations",
        Some(incident.clone()),
    )
    .await;
    assert_eq!(status, 403);
    let (status, got) = call(
        &admin,
        Method::POST,
        "/api/admin/screening/observations",
        Some(incident),
    )
    .await;
    assert_eq!(status, 201, "{got}");
    let id = got["id"].as_str().unwrap().to_string();

    for (body, code) in [
        (
            json!({"source": "user_junk", "kind": "ip", "value": "1.2.3.4", "stance": "deny", "reason": "x"}),
            "source_not_by_hand",
        ),
        (
            json!({"source": "manual", "kind": "ip", "value": "1.2.3.4", "stance": "deny", "reason": " "}),
            "reason_required",
        ),
        (
            json!({"source": "manual", "kind": "ip", "value": "", "stance": "deny", "reason": "x"}),
            "invalid_observation",
        ),
        (
            json!({"source": "manual", "kind": "ip", "value": "", "stance": "retract", "reason": "x"}),
            "invalid_observation",
        ),
    ] {
        let (status, got) = call(
            &admin,
            Method::POST,
            "/api/admin/screening/observations",
            Some(body),
        )
        .await;
        assert_eq!((status, got["error"].as_str()), (400, Some(code)));
    }
    let retract = json!({"source": "manual", "kind": "ip", "value": "", "stance": "retract", "retracts": id, "reason": "recorded by mistake"});
    let (status, _) = call(
        &admin,
        Method::POST,
        "/api/admin/screening/observations",
        Some(retract),
    )
    .await;
    assert_eq!(status, 201);

    let hash =
        json!({"sha256": "AB".repeat(32), "stance": "allow", "reason": "the vendor's installer"});
    let (status, _) = call(
        &admin,
        Method::POST,
        "/api/admin/screening/hashes",
        Some(hash),
    )
    .await;
    assert_eq!(status, 201);
    let bad = json!({"sha256": "not-hex", "stance": "deny", "reason": "x"});
    let (status, _) = call(
        &admin,
        Method::POST,
        "/api/admin/screening/hashes",
        Some(bad),
    )
    .await;
    assert_eq!(status, 400);

    let (status, list) = call(
        &admin,
        Method::GET,
        "/api/admin/screening/observations",
        None,
    )
    .await;
    assert_eq!(status, 200);
    let obs = list["observations"].as_array().unwrap();
    assert_eq!(
        obs.len(),
        2,
        "facts are kept, the retraction beside what it withdraws"
    );
    assert_eq!(obs[0]["stance"], "retract");
    assert_eq!(obs[1]["actor"], ADMIN);
    assert_eq!(list["hashes"][0]["sha256"], "ab".repeat(32));
    let (status, _) = call(
        &user,
        Method::GET,
        "/api/admin/screening/observations",
        None,
    )
    .await;
    assert_eq!(status, 403);
}
