//! Under a retention of 0 days, deleting an account removes everything it
//! owned at once: the maildir, its rows in the mail store, its keys in the
//! shared store and its group memberships. The other owner of a
//! conversation it shared keeps all of hers.
//!
//! Before the purge, the delete moved the maildir aside and dropped the
//! account record; every conversation row, uid map, ledger row and
//! shared-store key of the address stayed, for whoever took it next.

mod purge_support;

use purge_support::*;

#[tokio::test]
async fn a_deleted_account_leaves_nothing_and_the_other_owner_keeps_hers() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_DELETED_ACCOUNT_DAYS", "0") };
    let f = fixture().await;
    let mut mail = mail_conn(&f.state);
    let mut net = kevy_client::Connection::connect(&f.kevy_url).unwrap();
    let bob_rows = names(&snapshot(&mut mail, "*bob@x.com*"));
    assert!(
        bob_rows.iter().any(|k| k.starts_with("mailrs:threaduser:")),
        "the fixture needs bob's conversations: {bob_rows:?}"
    );
    let alice_mail = snapshot(&mut mail, "*alice@x.com*");
    let alice_net = snapshot(&mut net, "*alice@x.com*");

    let (status, _) = call(
        &f.state,
        "DELETE",
        &format!("/v1/admin/accounts/{BOB}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");

    let left_mail = names(&snapshot(&mut mail, "*bob@x.com*"));
    assert!(
        left_mail.is_empty(),
        "bob's mail-store rows remain: {left_mail:?}"
    );
    let left_net = names(&snapshot(&mut net, "*bob@x.com*"));
    assert!(
        left_net.is_empty(),
        "bob's shared-store keys remain: {left_net:?}"
    );
    let groups =
        mailrs_core_sidestate::families::permission_groups::group_members(&mut net, 1).unwrap();
    assert_eq!(groups, [ALICE], "bob is still in the group");
    assert!(
        parked(&f.root).is_empty(),
        "bob's parked maildir remains: {:?}",
        parked(&f.root)
    );
    assert!(!f.root.join("x.com/bob").exists());

    assert_eq!(snapshot(&mut mail, "*alice@x.com*"), alice_mail);
    assert_eq!(snapshot(&mut net, "*alice@x.com*"), alice_net);
    assert!(
        f.state
            .mailbox
            .get_thread_for_user(ALICE, &f.shared_tid)
            .unwrap()
            .is_some(),
        "alice lost the conversation she shared with bob"
    );
    let owners = f
        .state
        .mailbox
        .thread_message_owners(&f.shared_tid)
        .unwrap();
    assert!(
        owners.iter().all(|(u, _)| u == ALICE) && !owners.is_empty(),
        "{owners:?}"
    );
    assert!(f.root.join("x.com/alice").exists());
}
