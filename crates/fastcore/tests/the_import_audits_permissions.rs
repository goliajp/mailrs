//! The import tool writes an account's permissions straight into the
//! store, the one way to make an administrator that passes neither webapi
//! nor a core route: each grant or change leaves an audit line under the
//! actor `host`, and a line that cannot be written fails the import and
//! leaves the permissions as they were.

use std::io::Write;
use std::process::{Command, Stdio};

use mailrs_core_sidestate::families::audit::AuditFact;

const ADDRESS: &str = "ops@example.test";

/// Run the import over `input` against the store in `dir`, with the shared
/// store at `kevy_url`; its exit status.
fn import(dir: &std::path::Path, kevy_url: &str, input: &str) -> std::process::ExitStatus {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mailrs-fastcore-migrate"))
        .env("MAILRS_KEVY_DATA_DIR", dir)
        .env("MAILRS_KEVY_URL", kevy_url)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn the import");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("feed the import");
    child.wait().expect("the import ran")
}

fn grant(permissions: &[&str]) -> String {
    let blob = serde_json::json!({ "permissions": permissions, "is_super": false });
    format!(
        "{}\n",
        serde_json::json!({ "kind": "permissions", "address": ADDRESS, "blob": blob })
    )
}

/// The permissions the store in `dir` holds for [`ADDRESS`].
fn stored(dir: &std::path::Path) -> Option<serde_json::Value> {
    let store = kevy_embedded::Store::open(kevy_embedded::Config::default().with_persist(dir))
        .expect("open the imported store");
    mailrs_mailbox_kevy::KevyMailboxStore::new(std::sync::Arc::new(store))
        .get_permissions_blob(ADDRESS)
        .expect("read")
        .map(|j| serde_json::from_str(&j).expect("json"))
}

fn audit(kevy_url: &str) -> Vec<AuditFact> {
    let mut c = kevy_client::Connection::connect(kevy_url).expect("kevy");
    let mut got: Vec<AuditFact> = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit")
        .iter()
        .map(|r| serde_json::from_slice(r).expect("fact"))
        .collect();
    got.reverse();
    got
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_grant_or_change_is_audited_as_the_host() {
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("kevy");

    assert!(import(&dir, &url, &grant(&["admin.accounts"])).success());
    // the same permissions again change nothing and say nothing
    assert!(import(&dir, &url, &grant(&["admin.accounts"])).success());
    assert!(import(&dir, &url, &grant(&["admin.accounts", "admin.queue"])).success());

    let rows: Vec<(String, String, String)> = audit(&url)
        .into_iter()
        .map(|f| (f.actor, f.action, f.target))
        .collect();
    let row = || {
        (
            "host".to_string(),
            "account.permissions_set".to_string(),
            ADDRESS.to_string(),
        )
    };
    assert_eq!(rows, [row(), row()]);
    let details: Vec<String> = audit(&url).into_iter().map(|f| f.detail).collect();
    assert!(
        details[0].contains("admin.accounts") && details[1].contains("admin.queue"),
        "{details:?}"
    );
    assert_eq!(
        stored(&dir).expect("granted")["permissions"],
        serde_json::json!(["admin.accounts", "admin.queue"])
    );
}

#[test]
fn an_audit_line_that_cannot_be_written_fails_the_import_and_grants_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("kevy");
    // nothing listens on port 1
    let status = import(&dir, "kevy://127.0.0.1:1", &grant(&["admin.accounts"]));
    assert!(
        !status.success(),
        "the import passed without its audit line"
    );
    assert_eq!(stored(&dir), None, "permissions written with no audit line");
}
