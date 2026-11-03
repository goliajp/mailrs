//! The health answer says how many domains have a first-seen time and how
//! many were first seen in the last day, for a watch on how fast the set
//! grows.

mod live;

use mailrs_core_sidestate::families::domain_first;

const KEVY: &str = "mem://webapi-new-domains-in-health";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_counts_the_recorded_domains() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    let now = chrono::Utc::now().timestamp();
    let hosts = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    domain_first::record(&mut c, &hosts(&["old.example"]), now - 10 * 86_400).unwrap();
    domain_first::record(&mut c, &hosts(&["a.example", "b.example"]), now - 60).unwrap();

    let body: serde_json::Value = live
        .http
        .get(format!("{}/api/health", live.base))
        .send()
        .await
        .expect("health")
        .json()
        .await
        .expect("json");
    let d = &body["new_domains"];
    assert_eq!(d["keys"], 3, "{body}");
    assert_eq!(d["added_24h"], 2, "{body}");
    assert!(d["computed_at"].as_i64().is_some_and(|t| t >= now));
    assert_eq!(d["stale"], false, "{body}");
}
