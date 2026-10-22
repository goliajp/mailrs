//! Admin directory writes against a real in-process kevy: what a create
//! stores is what the list reports, and a delete leaves nothing behind.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Extension, Json, Path, State};
use axum::http::{Request, StatusCode};
use mailrs_core_sidestate::families::permission_groups::{GROUPS_KEY, members_key};
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

    // a permission group reports itself not builtin and claims no domain:
    // nothing limits its permissions to one
    let Json(created) = groups::create_group(Json(groups::CreateGroupRequest {
        name: "ops".into(),
        description: String::new(),
    }))
    .await
    .expect("create group");
    let Json(list) = groups::list_groups().await.expect("list");
    let row = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == created["id"])
        .cloned()
        .expect("listed");
    assert!(row["domain"].is_null());
    assert_eq!(row["is_builtin"], false);

    // a builtin group refuses deletion and keeps its members; an ordinary
    // one goes
    let mut c = kevy_client::Connection::connect("mem://webapi-admin-directory").unwrap();
    let builtin = serde_json::json!({ "id": 900, "name": "super", "is_builtin": true });
    c.hset(
        GROUPS_KEY.as_bytes(),
        &[(b"900".as_slice(), builtin.to_string().as_bytes())],
    )
    .unwrap();
    c.sadd(
        members_key(900).as_bytes(),
        &[b"root@example.test".as_slice()],
    )
    .unwrap();
    assert_eq!(
        groups::delete_group(Path(900)).await,
        Err(StatusCode::CONFLICT)
    );
    assert!(c.hget(GROUPS_KEY.as_bytes(), b"900").unwrap().is_some());
    assert_eq!(c.smembers(members_key(900).as_bytes()).unwrap().len(), 1);

    let ordinary = created["id"].as_i64().unwrap();
    assert_eq!(
        groups::delete_group(Path(ordinary)).await,
        Ok(StatusCode::NO_CONTENT)
    );
    let Json(list) = groups::list_groups().await.expect("list");
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["id"] != ordinary)
    );
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
