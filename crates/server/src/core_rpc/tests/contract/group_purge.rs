//! Purging a deleted email group's team state, alike on both cores: a dry
//! run counts the same families, the purge removes them and the group's
//! held claims, and a second pass has nothing to do.

use mailrs_core_sidestate::families::shared_inbox::{self, HeldClaim, notes};
use mailrs_core_sidestate::families::{email_groups, group_purge};

use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

const GID: i64 = 77;
const MEMBER: &str = "erin@example.test";

fn store(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).expect("side state")
}

/// A team with a conversation, a note and a claim, deleted long enough ago
/// for any retention.
fn deleted_team(url: &str) {
    let mut s = store(url);
    let row = serde_json::json!({ "id": GID, "address": "desk@example.test", "name": "desk" });
    s.hset(
        email_groups::GROUPS_KEY.as_bytes(),
        &[(GID.to_string().as_bytes(), row.to_string().as_bytes())],
    )
    .expect("group");
    let r = shared_inbox::conv_for_arrival(&mut s, GID, "<desk1@outside.example>", "", &[])
        .expect("conv");
    shared_inbox::record_copy(&mut s, r, MEMBER, "t1").expect("copy");
    notes::add(&mut s, r, MEMBER, "on it", 1).expect("note");
    let claim = HeldClaim {
        gid: GID,
        conv: r.conv,
        claim_id: "c1".into(),
    };
    shared_inbox::hold_claim(&mut s, MEMBER, "<desk-reply@example.test>", &claim).expect("claim");
    let long_ago = chrono::Utc::now().timestamp() - 400 * 86_400;
    assert!(group_purge::soft_delete(&mut s, GID, "admin@example.test", long_ago).expect("delete"));
}

fn team_keys(url: &str) -> usize {
    store(url)
        .keys(shared_inbox::group_glob(GID).as_bytes())
        .expect("keys")
        .len()
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        deleted_team(url);
    }
    let dry = d.both(
        "group_purge dry run",
        c.fastcore.group_purge(true, Some(GID)).await,
        c.pg_core.group_purge(true, Some(GID)).await,
        &[],
    );
    let (fc, _) = dry.expect("the dry run answers on both cores");
    assert_eq!(fc.rows.len(), 1, "{fc:?}");
    let counted: u64 = fc.rows[0].removed.iter().map(|p| p.count).sum();
    assert_eq!(counted, 6, "{fc:?}");
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        assert_eq!(team_keys(url), 5, "{url}: a dry run removed something");
    }

    let done = d.both(
        "group_purge",
        c.fastcore.group_purge(false, Some(GID)).await,
        c.pg_core.group_purge(false, Some(GID)).await,
        &[],
    );
    let (fc, _) = done.expect("the purge answers on both cores");
    assert_eq!(fc.rows.len(), 1, "{fc:?}");
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        assert_eq!(team_keys(url), 0, "{url}: team state left");
        let claims = store(url)
            .hgetall(shared_inbox::claims_key(MEMBER).as_bytes())
            .expect("claims");
        assert!(claims.is_empty(), "{url}: the group's claim is left");
    }

    let again = d.both(
        "group_purge once purged",
        c.fastcore.group_purge(false, None).await,
        c.pg_core.group_purge(false, None).await,
        &[],
    );
    let (fc, _) = again.expect("the second pass answers on both cores");
    assert_eq!((fc.pending, fc.due, fc.rows.len()), (0, 0, 0), "{fc:?}");
}
