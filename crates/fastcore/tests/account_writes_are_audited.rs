//! Creating and deleting an account through the core leaves one audit row
//! each in the shared store: under the actor the caller names, or `system`
//! when it names none.

use std::sync::Arc;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin as adm;
use mailrs_core_sidestate::families::audit::AuditFact;

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

/// (actor, action, target) of every row, oldest first.
fn rows(kv: &mut kevy_client::Connection) -> Vec<(String, String, String)> {
    let mut got: Vec<(String, String, String)> = kv
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<AuditFact>(r).unwrap())
        .map(|f| (f.actor, f.action, f.target))
        .collect();
    got.reverse();
    got
}

fn row(actor: &str, action: &str, target: &str) -> (String, String, String) {
    (actor.into(), action.into(), target.into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_caller_that_names_no_one_is_recorded_as_system() {
    const KEVY: &str = "mem://fastcore-account-audit-system";
    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    let core = serve(KEVY).await;
    core.add_account(&request("dan@example.test"))
        .await
        .expect("create");
    core.remove_account("dan@example.test")
        .await
        .expect("delete");
    assert_eq!(
        rows(&mut kv),
        [
            row("system", "account.create", "dan@example.test"),
            row("system", "account.delete", "dan@example.test"),
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_actor_is_recorded_once_and_not_on_failure() {
    const KEVY: &str = "mem://fastcore-account-audit-actor";
    // `+` and `&` survive the query string
    const ADMIN: &str = "ops+a&b@example.test";
    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    let core = serve(KEVY).await;
    core.add_account_by(&request("dan@example.test"), ADMIN)
        .await
        .expect("create");
    // creating over an existing address is refused, and audits nothing
    let again = core
        .add_account_by(&request("dan@example.test"), ADMIN)
        .await;
    assert_eq!(again.map_err(|e| e.status_code()), Err(409));
    core.remove_account_by("dan@example.test", ADMIN)
        .await
        .expect("delete");
    assert_eq!(
        rows(&mut kv),
        [
            row(ADMIN, "account.create", "dan@example.test"),
            row(ADMIN, "account.delete", "dan@example.test"),
        ]
    );
}
