//! Purging a deleted email group's team state, alike on both cores: a dry
//! run counts the same network families and, each in its own store, the
//! core's reply claims of the group's conversations; the purge removes
//! them and the group's held claims, and a second pass has nothing to do.

use mailrs_core_api::method::admin::PurgeCount;
use mailrs_core_api::method::shared::{ClaimReplyRequest, StateRequest};
use mailrs_core_sidestate::families::shared_inbox::{self, HeldClaim, notes};
use mailrs_core_sidestate::families::{email_groups, group_purge};

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

const GID: i64 = 77;

fn store(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).expect("side state")
}

/// A team of alice's with a conversation, a note and a held claim.
fn team(url: &str) -> shared_inbox::ConvRef {
    let mut s = store(url);
    let row = serde_json::json!({ "id": GID, "address": "desk@example.test", "name": "desk" });
    s.hset(
        email_groups::GROUPS_KEY.as_bytes(),
        &[(GID.to_string().as_bytes(), row.to_string().as_bytes())],
    )
    .expect("group");
    s.sadd(
        email_groups::members_key(GID).as_bytes(),
        &[ALICE.as_bytes()],
    )
    .expect("members");
    assert!(email_groups::set_shared(&mut s, GID, true).expect("shared"));
    let r = shared_inbox::conv_for_arrival(&mut s, GID, "<desk1@outside.example>", "", &[])
        .expect("conv");
    shared_inbox::record_copy(&mut s, r, ALICE, "t1").expect("copy");
    notes::add(&mut s, r, ALICE, "on it", 1).expect("note");
    let claim = HeldClaim {
        gid: GID,
        conv: r.conv,
        claim_id: "c1".into(),
    };
    shared_inbox::hold_claim(&mut s, ALICE, "<desk-reply@example.test>", &claim).expect("claim");
    r
}

/// Deleted long enough ago for any retention.
fn delete(url: &str) {
    let long_ago = chrono::Utc::now().timestamp() - 400 * 86_400;
    let mut s = store(url);
    assert!(group_purge::soft_delete(&mut s, GID, "admin@example.test", long_ago).expect("delete"));
}

fn team_keys(url: &str) -> usize {
    store(url)
        .keys(shared_inbox::group_glob(GID).as_bytes())
        .expect("keys")
        .len()
}

fn one(name: &str) -> Vec<PurgeCount> {
    vec![PurgeCount {
        name: name.into(),
        count: 1,
    }]
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let mut conv = 0;
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        conv = team(url).conv;
    }
    // the reply claim each core keeps in its own store
    let claim = ClaimReplyRequest {
        gid: GID,
        conv,
        seen_seq: 0,
        force: false,
        at: 1_790_000_000,
    };
    d.both(
        "shared_claim_reply on the team to delete",
        c.fastcore.shared_claim_reply(ALICE, &claim).await,
        c.pg_core.shared_claim_reply(ALICE, &claim).await,
        &["claim_id"],
    );
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        delete(url);
    }

    // the mail stores differ in kind: a kevy family against a table
    let dry = d.both(
        "group_purge dry run",
        c.fastcore.group_purge(true, Some(GID)).await,
        c.pg_core.group_purge(true, Some(GID)).await,
        &["mail_store"],
    );
    let (fc, pg) = dry.expect("the dry run answers on both cores");
    assert_eq!(fc.rows.len(), 1, "{fc:?}");
    let counted: u64 = fc.rows[0].network.iter().map(|p| p.count).sum();
    assert_eq!(counted, 6, "{fc:?}");
    assert_eq!(
        fc.rows[0].mail_store,
        one(mailrs_mailbox_kevy::SHARED_REPLY_FAMILY)
    );
    assert_eq!(pg.rows[0].mail_store, one("shared_reply_state"));
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        assert_eq!(team_keys(url), 5, "{url}: a dry run removed something");
    }

    let done = d.both(
        "group_purge",
        c.fastcore.group_purge(false, Some(GID)).await,
        c.pg_core.group_purge(false, Some(GID)).await,
        &["mail_store"],
    );
    let (fc, pg) = done.expect("the purge answers on both cores");
    assert_eq!(
        fc.rows[0].mail_store,
        one(mailrs_mailbox_kevy::SHARED_REPLY_FAMILY)
    );
    assert_eq!(pg.rows[0].mail_store, one("shared_reply_state"));
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        assert_eq!(team_keys(url), 0, "{url}: team state left");
        let claims = store(url)
            .hgetall(shared_inbox::claims_key(ALICE).as_bytes())
            .expect("claims");
        assert!(claims.is_empty(), "{url}: the group's claim is left");
    }

    // the reply count each core reads for the conversation is back to none;
    // asked as a member of a group with that id again
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        team(url);
    }
    let state = StateRequest { gid: GID, conv };
    let read = d.both(
        "shared_state once purged",
        c.fastcore.shared_state(ALICE, &state).await,
        c.pg_core.shared_state(ALICE, &state).await,
        &[],
    );
    let (fc, _) = read.expect("the state answers on both cores");
    assert_eq!(fc.reply.seq, 0, "the core's reply claim outlived the purge");
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        delete(url);
    }

    let again = d.both(
        "group_purge once purged",
        c.fastcore.group_purge(false, None).await,
        c.pg_core.group_purge(false, None).await,
        &["mail_store"],
    );
    let (fc, _) = again.expect("the second pass answers on both cores");
    assert_eq!(fc.rows.len(), 1, "the re-seeded group, purged");
    assert_eq!(fc.rows[0].mail_store, Vec::<PurgeCount>::new());
    let idle = d.both(
        "group_purge with nothing pending",
        c.fastcore.group_purge(false, None).await,
        c.pg_core.group_purge(false, None).await,
        &[],
    );
    let (fc, _) = idle.expect("an idle pass answers on both cores");
    assert_eq!((fc.pending, fc.due, fc.rows.len()), (0, 0, 0), "{fc:?}");
}
