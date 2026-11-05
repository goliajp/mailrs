//! A deleted account restored before its purge is what it was: the same
//! record (password hash, settings, permissions), every row of its mail,
//! its shared-store keys, its maildir where delivery finds it, and nothing
//! waiting for a purge any more. A restore is refused for an account that
//! exists, for an address not waiting for a purge, and when the address
//! was parked more than once and the request does not say which.

mod purge_support;

use mailrs_core_sidestate::families::account_purge::pending;
use mailrs_core_sidestate::families::audit::AuditFact;
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;
use purge_support::*;

fn files(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("x.com/bob")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.path().is_dir() {
                true => stack.push(e.path()),
                false => out.push(e.path().display().to_string()),
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn a_restored_account_is_what_it_was() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_DELETED_ACCOUNT_DAYS", "30") };
    let f = fixture().await;
    let mut mail = mail_conn(&f.state);
    let mut net = kevy_client::Connection::connect(&f.kevy_url).unwrap();
    let before_mail = snapshot(&mut mail, "*bob@x.com*");
    let before_net = snapshot(&mut net, "*bob@x.com*");
    let before_files = files(&f.root);
    assert!(!before_files.is_empty());
    let restore = format!("/v1/admin/accounts/{BOB}/restore");
    let by = serde_json::json!({ "by": "admin@x.com" });

    let (status, body) = call(&f.state, "POST", &restore, Some(by.clone())).await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT, "{body}");
    assert_eq!(body["refused"], "live", "{body}");

    let (status, _) = call(
        &f.state,
        "DELETE",
        &format!("/v1/admin/accounts/{BOB}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_ne!(
        snapshot(&mut mail, "*bob@x.com*"),
        before_mail,
        "the delete changed nothing"
    );

    // a second parked maildir, as an earlier deletion of the address left
    std::fs::create_dir_all(f.root.join(".deleted/x.com/bob.1000")).unwrap();
    let (status, body) = call(&f.state, "POST", &restore, Some(by.clone())).await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT, "{body}");
    assert_eq!(body["refused"], "several_parked", "{body}");
    let parked: Vec<String> = serde_json::from_value(body["parked"].clone()).unwrap();
    assert_eq!(parked.len(), 2, "{parked:?}");
    let ours = parked.iter().find(|p| *p != "1000").unwrap().clone();

    let pick = serde_json::json!({ "by": "admin@x.com", "parked": ours });
    let (status, body) = call(&f.state, "POST", &restore, Some(pick)).await;
    assert!(status.is_success(), "{status} {body}");

    assert_eq!(snapshot(&mut mail, "*bob@x.com*"), before_mail);
    assert_eq!(snapshot(&mut net, "*bob@x.com*"), before_net);
    assert_eq!(files(&f.root), before_files);
    assert!(pending::deleted_at(&mut net, BOB).unwrap().is_none());
    assert!(
        f.root.join(".deleted/x.com/bob.1000").exists(),
        "the other parked maildir"
    );
    let restored: Vec<AuditFact> = net
        .lrange(AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice::<AuditFact>(r).ok())
        .filter(|a| a.action == "account.restore")
        .collect();
    assert_eq!(restored.len(), 1);
    assert_eq!(
        (restored[0].actor.as_str(), restored[0].target.as_str()),
        ("admin@x.com", BOB)
    );
    assert!(
        f.state
            .mailbox
            .get_thread_for_user(BOB, &f.shared_tid)
            .unwrap()
            .is_some()
    );

    let (status, body) = call(&f.state, "POST", &restore, Some(by)).await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT, "{body}");
    assert_eq!(body["refused"], "live", "{body}");
}
