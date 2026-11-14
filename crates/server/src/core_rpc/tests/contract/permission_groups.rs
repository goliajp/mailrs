//! Permission groups live in the network kevy both cores share; an
//! account's effective permissions are its own plus every group's it is in.

use mailrs_core_sidestate::families::permission_groups;

use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};
use super::{ALICE, DOMAIN};

const GRANTED: &str = "admin.accounts";

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let group_only = format!("auditor@{DOMAIN}");
    let ids = [FASTCORE_KEVY, PG_CORE_KEVY].map(|kevy| put_group(kevy, &[ALICE, &group_only]));

    let alice = d.both(
        "effective_permissions in a group",
        fc.effective_permissions(ALICE).await,
        pg.effective_permissions(ALICE).await,
        &["created_at"],
    );
    if let Some((a, b)) = alice {
        for (core, p) in [("fastcore", a), ("pg-core", b)] {
            if !p.permissions.iter().any(|x| x == GRANTED) || p.groups.len() != 1 {
                d.0.push(format!("{core}: the group's grant is missing: {p:?}"));
            }
        }
    }
    // not an account, only a group member: still answered, with the grant
    d.both(
        "effective_permissions of a group-only address",
        fc.effective_permissions(&group_only).await,
        pg.effective_permissions(&group_only).await,
        &["created_at"],
    );
    d.both(
        "effective_permissions of an unknown address",
        fc.effective_permissions(&format!("nobody@{DOMAIN}")).await,
        pg.effective_permissions(&format!("nobody@{DOMAIN}")).await,
        &[],
    );

    for (kevy, id) in [FASTCORE_KEVY, PG_CORE_KEVY].into_iter().zip(ids) {
        let mut conn = kevy_client::Connection::connect(kevy).expect("kevy");
        permission_groups::delete(&mut conn, id).expect("drop group");
    }
    d.both(
        "effective_permissions after the group is gone",
        fc.effective_permissions(ALICE).await,
        pg.effective_permissions(ALICE).await,
        &[],
    );
}

fn put_group(kevy: &str, members: &[&str]) -> i64 {
    let mut conn = kevy_client::Connection::connect(kevy).expect("kevy");
    let row = permission_groups::create(&mut conn, "auditors", "", 1_700_000_000).expect("group");
    let id = row["id"].as_i64().expect("group id");
    permission_groups::set_permissions(&mut conn, id, &[GRANTED.to_string()]).expect("grant");
    for m in members {
        permission_groups::add_member(&mut conn, id, m).expect("member");
    }
    id
}
