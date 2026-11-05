//! A deleted group's purge on fastcore takes the reply claims of its
//! conversations from the embedded store with its team state from the
//! network kevy, and counts both in one audit row; a dry run counts and
//! keeps them, and another group's claims stay.

mod purge_support;

use mailrs_core_api::method::admin::{GroupPurgeResponse, PATH_GROUP_PURGE, PurgeCount};
use mailrs_core_sidestate::families::account_purge::pending;
use mailrs_core_sidestate::families::{email_groups, group_purge, shared_inbox};
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;
use mailrs_mailbox::shared_reply::Claim;
use purge_support::*;

const GID: i64 = 9;

async fn purge(f: &Fixture, query: &str) -> GroupPurgeResponse {
    let (status, body) = call(
        &f.state,
        "POST",
        &format!("{PATH_GROUP_PURGE}{query}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    serde_json::from_value(body).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_groups_reply_claims_go_with_its_team_state() {
    let f = fixture().await;
    // SAFETY: the only test in this binary; nothing else reads it meanwhile
    unsafe { std::env::set_var(pending::RETENTION_ENV, "30") };
    let mut net = kevy_client::Connection::connect(&f.kevy_url).unwrap();
    let row = serde_json::json!({ "id": GID, "address": "desk@x.com", "name": "desk" });
    net.hset(
        email_groups::GROUPS_KEY.as_bytes(),
        &[(GID.to_string().as_bytes(), row.to_string().as_bytes())],
    )
    .unwrap();
    let r = shared_inbox::conv_for_arrival(&mut net, GID, "<desk1@y.com>", "", &[]).unwrap();
    for tag in [r.tag(), format!("{}:1", GID * 10)] {
        let won = f
            .state
            .mailbox
            .claim_reply(&tag, ALICE, 0, false, 1)
            .unwrap();
        assert!(matches!(won, Claim::Won { .. }), "{tag}");
    }
    let long_ago = chrono::Utc::now().timestamp() - 31 * 86_400;
    assert!(group_purge::soft_delete(&mut net, GID, "admin@x.com", long_ago).unwrap());

    let replies = vec![PurgeCount {
        name: mailrs_mailbox_kevy::SHARED_REPLY_FAMILY.into(),
        count: 1,
    }];
    let dry = purge(&f, &format!("?gid={GID}")).await;
    assert!(dry.dry_run);
    assert_eq!(dry.rows.len(), 1, "{dry:?}");
    assert_eq!(dry.rows[0].mail_store, replies);
    assert_eq!(
        f.state.mailbox.reply_state(&r.tag()).unwrap().seq,
        1,
        "a dry run removed it"
    );

    let done = purge(&f, &format!("?dry_run=false&gid={GID}")).await;
    assert_eq!(
        done.rows, dry.rows,
        "the purge removes what the dry run counted"
    );
    assert_eq!(f.state.mailbox.reply_state(&r.tag()).unwrap().seq, 0);
    assert_eq!(
        f.state
            .mailbox
            .reply_state(&format!("{}:1", GID * 10))
            .unwrap()
            .seq,
        1,
        "another group's claim stays"
    );
    assert!(
        net.keys(shared_inbox::group_glob(GID).as_bytes())
            .unwrap()
            .is_empty()
    );
    let audit: Vec<serde_json::Value> = net
        .lrange(AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|v| serde_json::from_slice::<serde_json::Value>(v).ok())
        .filter(|v| v["action"] == group_purge::PURGED)
        .collect();
    assert_eq!(audit.len(), 1);
    let detail: serde_json::Value =
        serde_json::from_str(audit[0]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(
        detail["mail_store"],
        serde_json::to_value(&replies).unwrap()
    );
    assert!(!detail["network"].as_array().unwrap().is_empty());

    let again = purge(&f, "?dry_run=false").await;
    assert_eq!((again.pending, again.due, again.rows.len()), (0, 0, 0));
}
