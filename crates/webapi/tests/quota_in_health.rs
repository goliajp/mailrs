//! The health answer says how full the fullest account is and how many are
//! past 80% of their limit, so a catch-all filling up is seen before it
//! starts refusing mail; and how many accounts have no limit at all.

mod live;

use mailrs_core_api::method::admin::DEFAULT_QUOTA_BYTES;
use mailrs_core_sidestate::keys::quota::{limit_key, used_key};

const KEVY: &str = "mem://webapi-quota-in-health";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_reports_the_fullest_account_and_those_without_a_limit() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in ["full@q.test", "light@q.test", "bare@q.test"] {
        live.add_account(who, "pw-pw-pw-pw").await;
    }
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    let nine_tenths = DEFAULT_QUOTA_BYTES / 10 * 9;
    c.set(
        used_key("full@q.test").as_bytes(),
        nine_tenths.to_string().as_bytes(),
    )
    .unwrap();
    c.set(used_key("light@q.test").as_bytes(), b"1024").unwrap();
    // an account from before every creation wrote a limit
    c.del(&[limit_key("bare@q.test").as_bytes()]).unwrap();

    let body: serde_json::Value = live
        .http
        .get(format!("{}/api/health", live.base))
        .send()
        .await
        .expect("health")
        .json()
        .await
        .expect("json");
    let q = &body["quota"];
    assert_eq!(q["accounts"], 3, "{body}");
    assert_eq!(q["accounts_without_limit"], 1, "{body}");
    assert_eq!(q["over_80"], 1, "{body}");
    let ratio = q["max_used_ratio"].as_f64().expect("a ratio");
    assert!((ratio - 0.9).abs() < 1e-9, "{body}");
    assert_eq!(q["stale"], false, "{body}");
}
