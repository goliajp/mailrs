//! Permission groups take part in authorization: webapi's checks ask a real
//! fastcore, which reads the groups webapi's admin handlers write.

use std::sync::Arc;

use axum::extract::{Extension, Json, Path, State};
use axum::http::StatusCode;
use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin as adm;
use mailrs_webapi::WebState;
use mailrs_webapi::handlers::admin_directory as dir;
use mailrs_webapi::handlers::conversations::AuthedUser;
use mailrs_webapi::handlers::groups;
use mailrs_webapi::handlers::kevy_util::{require_admin, require_permission};

const KEVY: &str = "mem://webapi-permission-groups";
const ALICE: &str = "alice@example.test";

async fn serve_fastcore() -> Client {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(KEVY.to_string());
    let router = mailrs_fastcore::build_router(Arc::new(state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    Client::new(format!("http://{addr}"), "secret")
}

fn web_state(core: Client) -> Arc<WebState> {
    Arc::new(WebState {
        core: Arc::new(core),
        bind_addr: "127.0.0.1:0".into(),
        event_bus: std::sync::OnceLock::new(),
        trace_bus: std::sync::OnceLock::new(),
        receiver_stats: Arc::new(mailrs_webapi::handlers::smtp_monitor::StatsCache::default()),
        started_at: std::time::Instant::now(),
        llm_config: None,
    })
}

async fn account_groups(st: &Arc<WebState>) -> serde_json::Value {
    let Json(v) = dir::list_account_groups(
        State(st.clone()),
        Extension(AuthedUser("admin@example.test".into())),
        Path(ALICE.into()),
    )
    .await
    .expect("groups");
    v
}

async fn join(id: i64) {
    groups::add_group_member(
        Path(id),
        Json(groups::AddGroupMemberRequest {
            address: ALICE.into(),
        }),
    )
    .await
    .expect("add member");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_group_grants_its_permissions_until_the_member_or_group_goes() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let core = serve_fastcore().await;
    core.add_account(&adm::AddAccountRequest {
        address: ALICE.into(),
        display_name: String::new(),
        password: "pw".into(),
    })
    .await
    .expect("create");
    let st = web_state(core);
    let check = || require_permission(&st, ALICE, "admin.domains");

    assert_eq!(check().await, Err(StatusCode::FORBIDDEN));
    assert_eq!(require_admin(&st, ALICE).await, Err(StatusCode::FORBIDDEN));
    assert_eq!(
        account_groups(&st).await,
        serde_json::json!({ "groups": [] })
    );

    let Json(g) = groups::create_group(Json(groups::CreateGroupRequest {
        name: "domain-admins".into(),
        description: String::new(),
    }))
    .await
    .expect("create group");
    let id = g["id"].as_i64().expect("id");
    groups::set_group_permissions(
        Path(id),
        Json(groups::SetGroupPermissionsRequest {
            permissions: vec!["admin.domains".into()],
        }),
    )
    .await
    .expect("set permissions");
    assert_eq!(
        check().await,
        Err(StatusCode::FORBIDDEN),
        "not a member yet"
    );

    join(id).await;
    assert_eq!(check().await, Ok(()));
    assert_eq!(require_admin(&st, ALICE).await, Ok(()));
    assert_eq!(
        account_groups(&st).await,
        serde_json::json!({ "groups": ["domain-admins"] })
    );
    assert_eq!(
        require_permission(&st, ALICE, "admin.accounts").await,
        Err(StatusCode::FORBIDDEN),
        "only what the group holds"
    );

    groups::remove_group_member(Path((id, ALICE.into())))
        .await
        .expect("remove member");
    assert_eq!(check().await, Err(StatusCode::FORBIDDEN));
    assert_eq!(
        account_groups(&st).await,
        serde_json::json!({ "groups": [] })
    );

    join(id).await;
    assert_eq!(check().await, Ok(()));
    groups::delete_group(Path(id)).await.expect("delete");
    assert_eq!(check().await, Err(StatusCode::FORBIDDEN));
    assert_eq!(
        account_groups(&st).await,
        serde_json::json!({ "groups": [] })
    );
}
