//! The RCPT check answers "deliverable" for exactly the addresses the drain
//! delivers to — a maildir, an account, an email group, an alias, a domain
//! catch-all — and "unknown" for the rest, without writing anything. A
//! store it cannot read is an error, never "unknown". Deleting an account
//! moves its maildir aside, after which the address is unknown again.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use kevy_client::Connection;
use mailrs_alias_store::AliasStore;
use mailrs_core_api::client::Client;
use mailrs_core_sidestate::families::email_groups::{GROUPS_KEY, members_key};
use mailrs_fastcore::FastcoreState;

const UNREACHABLE: &str = "kevy://127.0.0.1:1";

// fastcore reads the maildir root from the environment, once per process
fn maildir_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = tempfile::tempdir().unwrap().keep().join("maildir");
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY: set once, before any test in this binary reads it
        unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
        root
    })
}

fn maildir(addr: &str) -> PathBuf {
    let (local, domain) = addr.split_once('@').unwrap();
    maildir_root().join(domain).join(local)
}

fn make_maildir(addr: &str) {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(maildir(addr).join(sub)).unwrap();
    }
}

fn store() -> mailrs_mailbox_kevy::KevyMailboxStore {
    mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ))
}

fn account(mailbox: &mailrs_mailbox_kevy::KevyMailboxStore, addr: &str) {
    let blob = serde_json::json!({ "address": addr, "password_hash": "x" }).to_string();
    mailbox.upsert_account(addr, &blob).unwrap();
}

async fn serve(state: FastcoreState) -> Client {
    maildir_root();
    let router = mailrs_fastcore::build_router(Arc::new(state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    Client::new(base, "secret")
}

#[tokio::test]
async fn every_kind_of_recipient_the_drain_delivers_to_is_deliverable() {
    let net = "mem://recipient-check-kinds";
    let d = "kinds.example";
    let mailbox = store();
    account(&mailbox, &format!("acct@{d}"));
    account(&mailbox, "target@catchall.example");
    make_maildir(&format!("dironly@{d}"));
    mailbox
        .upsert(&format!("alias@{d}"), &format!("acct@{d}"))
        .unwrap();
    mailbox
        .upsert(&format!("dangling@{d}"), &format!("nobody@{d}"))
        .unwrap();
    mailbox
        .upsert("@catchall.example", "target@catchall.example")
        .unwrap();
    let mut kv = Connection::connect(net).unwrap();
    let row = serde_json::to_vec(&serde_json::json!({ "id": 1, "address": format!("team@{d}") }))
        .unwrap();
    kv.hset(GROUPS_KEY.as_bytes(), &[(b"1".as_slice(), row.as_slice())])
        .unwrap();
    kv.sadd(members_key(1).as_bytes(), &[format!("acct@{d}").as_bytes()])
        .unwrap();
    let mut state = FastcoreState::new(mailbox);
    state.net_url = Some(net.to_string());
    let core = serve(state).await;

    for addr in [
        format!("acct@{d}"),
        format!("ACCT@{d}"),
        format!("dironly@{d}"),
        format!("team@{d}"),
        format!("alias@{d}"),
        "anyone@catchall.example".to_string(),
    ] {
        assert!(core.recipient_deliverable(&addr).await.unwrap(), "{addr}");
    }
    for addr in [format!("nobody@{d}"), format!("dangling@{d}")] {
        assert!(!core.recipient_deliverable(&addr).await.unwrap(), "{addr}");
    }
    // asking wrote nothing: the account still has no maildir
    assert!(!maildir(&format!("acct@{d}")).exists());
}

#[tokio::test]
async fn an_unreadable_store_is_an_error_not_an_unknown_recipient() {
    let d = "errors.example";
    let mailbox = store();
    account(&mailbox, &format!("acct@{d}"));
    let mut state = FastcoreState::new(mailbox.clone());
    state.net_url = Some(UNREACHABLE.to_string());
    let core = serve(state).await;
    assert!(
        core.recipient_deliverable(&format!("nobody@{d}"))
            .await
            .is_err()
    );
    // the recipient's own mailbox is found before the network kevy is needed
    assert!(
        core.recipient_deliverable(&format!("acct@{d}"))
            .await
            .unwrap()
    );

    let alias_store = Arc::new(mailrs_alias_store_net::NetworkKevyAliasStore::new(
        UNREACHABLE,
    ));
    let state = FastcoreState::new_with_alias_store(mailbox, alias_store);
    assert_eq!(state.net_url, None);
    let core = serve(state).await;
    assert!(
        core.recipient_deliverable(&format!("nobody@{d}"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_deleted_account_is_no_longer_a_mailbox() {
    let d = "deleted.example";
    let addr = format!("gone@{d}");
    let mailbox = store();
    account(&mailbox, &addr);
    account(&mailbox, &format!("fresh@{d}"));
    account(&mailbox, &format!("catch@{d}"));
    make_maildir(&addr);
    make_maildir(&format!("catch@{d}"));
    std::fs::write(
        maildir(&addr).join("new/1.msg"),
        b"Subject: kept\r\n\r\nkept",
    )
    .unwrap();
    let core = serve(FastcoreState::new(mailbox.clone())).await;
    assert!(core.recipient_deliverable(&addr).await.unwrap());

    core.remove_account(&addr).await.unwrap();
    assert!(!maildir(&addr).exists());
    assert_eq!(mailbox.get_account_blob(&addr).unwrap(), None);
    let set_aside: Vec<_> = std::fs::read_dir(maildir_root().join(".deleted").join(d))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(set_aside.len(), 1, "{set_aside:?}");
    let name = set_aside[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("gone."), "{name}");
    assert!(set_aside[0].join("new/1.msg").is_file());
    assert!(!core.recipient_deliverable(&addr).await.unwrap());

    // with the maildir gone a catch-all takes the address like any other
    mailbox
        .upsert(&format!("@{d}"), &format!("catch@{d}"))
        .unwrap();
    assert!(core.recipient_deliverable(&addr).await.unwrap());

    // an account that never had a maildir is just removed
    core.remove_account(&format!("fresh@{d}")).await.unwrap();
    assert_eq!(
        mailbox.get_account_blob(&format!("fresh@{d}")).unwrap(),
        None
    );
}
