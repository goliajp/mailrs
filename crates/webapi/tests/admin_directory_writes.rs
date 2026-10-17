//! Admin directory writes against a real in-process kevy: what a create
//! stores is what the list reports, and a delete leaves nothing behind.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Extension, Json, Path, State};
use axum::http::{Request, StatusCode};
use mailrs_webapi::WebState;
use mailrs_webapi::handlers::admin_directory as dir;
use mailrs_webapi::handlers::conversations::AuthedUser;
use mailrs_webapi::handlers::groups;
use tower::ServiceExt;

fn state() -> Arc<WebState> {
    Arc::new(WebState {
        // nothing listens here: a handler that reaches the core fails
        core: Arc::new(mailrs_core_api::client::Client::new(
            "http://127.0.0.1:1",
            "test-secret",
        )),
        bind_addr: "127.0.0.1:0".into(),
        event_bus: std::sync::OnceLock::new(),
        trace_bus: std::sync::OnceLock::new(),
        receiver_stats: Arc::new(mailrs_webapi::handlers::smtp_monitor::StatsCache::default()),
        started_at: std::time::Instant::now(),
        llm_config: None,
    })
}

fn admin() -> Extension<AuthedUser> {
    Extension(AuthedUser("admin@example.test".into()))
}

async fn members(st: &Arc<WebState>, id: i64) -> serde_json::Value {
    let Json(v) = dir::list_email_group_members(State(st.clone()), admin(), Path(id.to_string()))
        .await
        .expect("members");
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn groups_store_what_they_are_given_and_deletes_leave_nothing() {
    // SAFETY: the only test in this binary that reads kevy, before any read
    unsafe { std::env::set_var("MAILRS_KEVY_URL", "mem://webapi-admin-directory") };
    let st = state();

    // an email group's initial members land in the member set
    let Json(g) = groups::create_email_group(Json(groups::CreateEmailGroupRequest {
        address: "team@example.test".into(),
        domain: "example.test".into(),
        name: "Team".into(),
        description: String::new(),
        members: vec!["a@example.test".into()],
    }))
    .await
    .expect("create");
    let id = g["id"].as_i64().expect("id");
    assert!(
        g.get("members").is_none(),
        "members are not copied into the row"
    );
    assert_eq!(
        members(&st, id).await["items"],
        serde_json::json!(["a@example.test"])
    );

    dir::add_email_group_member(
        State(st.clone()),
        admin(),
        Path(id.to_string()),
        Json(dir::AddMemberRequest {
            address: "b@example.test".into(),
        }),
    )
    .await
    .expect("add member");
    let mut now: Vec<String> =
        serde_json::from_value(members(&st, id).await["items"].clone()).expect("strings");
    now.sort();
    assert_eq!(now, ["a@example.test", "b@example.test"]);

    // deleting the group deletes its members, so a later group with the
    // same id cannot inherit them
    groups::delete_email_group(Path(id)).await.expect("delete");
    assert_eq!(members(&st, id).await["items"], serde_json::json!([]));

    // a permission group keeps its domain and reports itself not builtin
    let Json(scoped) = groups::create_group(Json(groups::CreateGroupRequest {
        name: "ops".into(),
        description: String::new(),
        domain: Some("example.test".into()),
    }))
    .await
    .expect("create group");
    let Json(global) = groups::create_group(Json(groups::CreateGroupRequest {
        name: "all".into(),
        description: String::new(),
        domain: Some(String::new()),
    }))
    .await
    .expect("create group");
    let Json(list) = groups::list_groups().await.expect("list");
    let row = |id: &serde_json::Value| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == *id)
            .cloned()
            .expect("listed")
    };
    let scoped = row(&scoped["id"]);
    assert_eq!(scoped["domain"], "example.test");
    assert_eq!(scoped["is_builtin"], false);
    assert!(row(&global["id"])["domain"].is_null(), "empty means global");
}

#[tokio::test]
async fn an_empty_password_is_refused_before_the_core_is_asked() {
    let st = state();
    let changed = dir::set_account_password(
        State(st.clone()),
        admin(),
        Path("qa@example.test".into()),
        Json(dir::SetAccountPasswordRequest {
            password: String::new(),
        }),
    )
    .await;
    assert_eq!(changed, Err(StatusCode::BAD_REQUEST));

    let created = dir::add_account(
        State(st),
        admin(),
        Json(mailrs_core_api::method::admin::AddAccountRequest {
            address: "qa@example.test".into(),
            display_name: "QA".into(),
            password: String::new(),
        }),
    )
    .await;
    assert_eq!(created, Err(StatusCode::BAD_REQUEST));
}

#[tokio::test]
async fn the_password_route_exists_behind_the_session_gate() {
    let router = mailrs_webapi::build_router(state());
    let res = router
        .oneshot(
            Request::post("/api/admin/accounts/qa%40example.test/password")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
