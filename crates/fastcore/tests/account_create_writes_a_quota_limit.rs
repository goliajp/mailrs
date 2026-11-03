//! Creating an account gives it the default quota limit where the receiver
//! reads it, and a shared store that is configured but unreachable fails
//! the creation instead of leaving an account with no limit.

use std::sync::Arc;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin as adm;
use mailrs_core_sidestate::families::quota_limits::limit;

async fn serve(net_url: &str) -> Client {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(net_url.to_string());
    let router = mailrs_fastcore::build_router(Arc::new(state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    Client::new(format!("http://{addr}"), "secret")
}

fn request(address: &str) -> adm::AddAccountRequest {
    adm::AddAccountRequest {
        address: address.into(),
        display_name: String::new(),
        password: "pw".into(),
        password_hash: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_created_account_has_the_default_limit() {
    const KEVY: &str = "mem://fastcore-account-create-quota";
    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    let core = serve(KEVY).await;
    core.add_account(&request("dan@example.test"))
        .await
        .expect("create");
    assert_eq!(
        limit(&mut kv, "dan@example.test").unwrap(),
        Some(adm::DEFAULT_QUOTA_BYTES)
    );
    let account = core
        .get_account_with_hash("dan@example.test")
        .await
        .expect("account");
    assert_eq!(account.public.quota_bytes, adm::DEFAULT_QUOTA_BYTES);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_shared_store_creates_no_account() {
    // nothing listens on port 1
    let core = serve("kevy://127.0.0.1:1").await;
    let got = core.add_account(&request("eve@example.test")).await;
    assert_eq!(got.map_err(|e| e.status_code()), Err(503));
    let after = core.get_account_with_hash("eve@example.test").await;
    assert_eq!(after.map(|_| ()).map_err(|e| e.status_code()), Err(404));
}
