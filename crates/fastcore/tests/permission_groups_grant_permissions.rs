//! A permission group's permissions reach its members' effective
//! permissions, and stop reaching them when the member is removed or the
//! group is deleted.

use std::sync::Arc;

use kevy_client::Connection;
use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin as adm;
use mailrs_core_sidestate::families::permission_groups::{
    GROUPS_KEY, members_key, permissions_key,
};

async fn serve_fastcore(net_url: &str) -> Client {
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

async fn create(core: &Client, address: &str) {
    core.add_account(&adm::AddAccountRequest {
        address: address.into(),
        display_name: String::new(),
        password: "pw".into(),
        password_hash: None,
    })
    .await
    .expect("create");
}

fn put_group(kv: &mut Connection, id: i64, perms: &[&str]) {
    let row = serde_json::json!({ "id": id, "name": "domain-admins" }).to_string();
    kv.hset(
        GROUPS_KEY.as_bytes(),
        &[(id.to_string().as_bytes(), row.as_bytes())],
    )
    .unwrap();
    let p: Vec<&[u8]> = perms.iter().map(|s| s.as_bytes()).collect();
    kv.sadd(permissions_key(id).as_bytes(), &p).unwrap();
}

async fn perms(core: &Client, address: &str) -> adm::EffectivePermissionsResponse {
    core.effective_permissions(address).await.expect("perms")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn membership_grants_and_removal_revokes() {
    const KEVY: &str = "mem://fastcore-permission-groups";
    let core = serve_fastcore(KEVY).await;
    let mut kv = Connection::connect(KEVY).unwrap();
    create(&core, "alice@example.test").await;
    assert!(
        perms(&core, "alice@example.test")
            .await
            .permissions
            .is_empty()
    );

    put_group(&mut kv, 7, &["admin.domains"]);
    kv.sadd(members_key(7).as_bytes(), &[b"alice@example.test"])
        .unwrap();
    let p = perms(&core, "alice@example.test").await;
    assert_eq!(p.permissions, ["admin.domains"]);
    assert_eq!(p.groups.iter().map(|g| g.id).collect::<Vec<_>>(), [7]);
    assert!(!p.is_super, "a group never makes an account super");

    kv.srem(members_key(7).as_bytes(), &[b"alice@example.test"])
        .unwrap();
    let p = perms(&core, "alice@example.test").await;
    assert!(p.permissions.is_empty() && p.groups.is_empty());

    // deleting a group the way the admin page does: the row and both sets
    kv.sadd(members_key(7).as_bytes(), &[b"alice@example.test"])
        .unwrap();
    assert_eq!(
        perms(&core, "alice@example.test").await.permissions,
        ["admin.domains"]
    );
    kv.hdel(GROUPS_KEY.as_bytes(), &[b"7"]).unwrap();
    kv.del(&[permissions_key(7).as_bytes(), members_key(7).as_bytes()])
        .unwrap();
    assert!(
        perms(&core, "alice@example.test")
            .await
            .permissions
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_address_without_its_own_blob_is_answered_from_its_groups() {
    const KEVY: &str = "mem://fastcore-permission-groups-no-blob";
    let core = serve_fastcore(KEVY).await;
    let mut kv = Connection::connect(KEVY).unwrap();
    put_group(&mut kv, 1, &["admin.queue", "mail.send"]);
    kv.sadd(members_key(1).as_bytes(), &[b"ops@example.test"])
        .unwrap();

    let p = perms(&core, "ops@example.test").await;
    assert_eq!(p.address, "ops@example.test");
    assert_eq!(p.permissions, ["admin.queue", "mail.send"]);
    assert!(!p.is_super);

    let none = core
        .effective_permissions("nobody@example.test")
        .await
        .expect_err("no blob, no group");
    assert_eq!(none.status_code(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_network_kevy_leaves_the_own_permissions() {
    // nothing listens on port 1
    let core = serve_fastcore("kevy://127.0.0.1:1").await;
    create(&core, "carol@example.test").await;
    let p = perms(&core, "carol@example.test").await;
    assert_eq!(p.address, "carol@example.test");
    assert!(p.permissions.is_empty());
}
