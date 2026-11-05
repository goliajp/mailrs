//! Both cores audit an account they create or delete: one row each, under
//! the actor the caller names, or `system` when it names none.

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_sidestate::families::audit::AuditFact;

use super::harness;

/// (actor, action, target) of every row in one core's shared store,
/// oldest first.
fn rows(url: &str) -> Vec<(String, String, String)> {
    let mut c = kevy_client::Connection::connect(url).expect("kevy");
    let mut got: Vec<(String, String, String)> = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit")
        .iter()
        .map(|r| serde_json::from_slice::<AuditFact>(r).expect("fact"))
        .map(|f| (f.actor, f.action, f.target))
        .collect();
    got.reverse();
    got
}

fn request(address: &str) -> AddAccountRequest {
    AddAccountRequest {
        address: address.into(),
        display_name: String::new(),
        password: "pw".into(),
        password_hash: None,
    }
}

async fn create_and_delete(core: &Client) {
    core.add_account(&request("dan@example.test"))
        .await
        .expect("create");
    core.remove_account("dan@example.test")
        .await
        .expect("delete");
    core.add_account_by(&request("eve@example.test"), "admin@example.test")
        .await
        .expect("create by");
    core.remove_account_by("eve@example.test", "admin@example.test")
        .await
        .expect("delete by");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_cores_audit_account_creation_and_deletion_alike() {
    const FC: &str = "mem://account-audit-fastcore";
    const PG: &str = "mem://account-audit-pg-core";
    let c = harness::start_on(FC, PG).await;
    create_and_delete(&c.fastcore).await;
    create_and_delete(&c.pg_core).await;
    let row = |actor: &str, action: &str, target: &str| {
        (actor.to_string(), action.to_string(), target.to_string())
    };
    let want = vec![
        row("system", "account.create", "dan@example.test"),
        row("system", "account.delete", "dan@example.test"),
        row("admin@example.test", "account.create", "eve@example.test"),
        row("admin@example.test", "account.delete", "eve@example.test"),
    ];
    assert_eq!(rows(FC), want, "fastcore");
    assert_eq!(rows(PG), want, "SQL core");
}
