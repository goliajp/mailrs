use super::super::hangup::{Hangup, seed};
use super::*;

async fn walked(state: &Arc<FastcoreState>, hangup: &Hangup) -> serde_json::Value {
    let q = ReplayQuery {
        skip: 0,
        limit: 100,
        days: 36_500,
        pause_ms: 0,
        scope: Scope::All,
    };
    let res = replay(state, &q, String::new(), hangup);
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn the_replay_stops_soon_after_the_caller_hangs_up() {
    let mut state = crate::tests::fresh_state_owned();
    state.net_url = Some("mem://replay-hangup".into());
    let state = Arc::new(state);
    seed(&state, "u@x", 5);
    let all = walked(&state, &Hangup::never()).await;
    assert_eq!(all["walked"], 5, "{all}");
    let cut = walked(&state, &Hangup::after(2)).await;
    assert_eq!(cut["walked"], 2, "{cut}");
    assert_eq!(cut["stopped_early"], true, "{cut}");
}
