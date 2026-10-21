//! Accounts, domains, aliases, permissions: the admin surface webapi and the
//! receiver call.

use mailrs_core_api::method::admin::{
    AddAccountRequest, SetPasswordRequest, SetQuotaRequest, UpdateAccountRequest,
    UpdateRecoveryEmailRequest,
};

use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};
use super::{ALICE, BOB, DOMAIN};

/// Ids and times each store mints for itself.
const MINTED: &[&str] = &["id", "created_at", "updated_at"];

pub(super) async fn seed(c: &Cores, d: &mut Diffs) {
    d.both(
        "add_domain",
        c.fastcore.add_domain(DOMAIN).await,
        c.pg_core.add_domain(DOMAIN).await,
        &[],
    );
    for (address, name) in [(ALICE, "Alice"), (BOB, "Bob")] {
        let req = AddAccountRequest {
            address: address.into(),
            display_name: name.into(),
            password: "pw".into(),
        };
        d.both(
            "add_account",
            c.fastcore.add_account(&req).await,
            c.pg_core.add_account(&req).await,
            &[],
        );
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);

    d.both(
        "list_domains",
        fc.list_domains().await,
        pg.list_domains().await,
        MINTED,
    );

    let req = UpdateAccountRequest {
        display_name: "Alice A.".into(),
    };
    d.both(
        "update_account",
        fc.update_account(ALICE, &req).await,
        pg.update_account(ALICE, &req).await,
        &[],
    );
    let req = SetQuotaRequest {
        quota_bytes: 1 << 30,
    };
    d.both(
        "set_quota",
        fc.set_quota(ALICE, &req).await,
        pg.set_quota(ALICE, &req).await,
        &[],
    );
    let req = UpdateRecoveryEmailRequest {
        recovery_email: "alice@elsewhere.test".into(),
    };
    d.both(
        "set_recovery_email",
        fc.set_recovery_email(ALICE, &req).await,
        pg.set_recovery_email(ALICE, &req).await,
        &[],
    );
    let hash = mailrs_core::users::UserStore::hash_password("new-pw").expect("hash");
    let req = SetPasswordRequest {
        password_hash: hash,
    };
    d.both(
        "set_account_password",
        fc.set_account_password(ALICE, &req).await,
        pg.set_account_password(ALICE, &req).await,
        &[],
    );
    d.both(
        "list_accounts",
        fc.list_accounts().await,
        pg.list_accounts().await,
        MINTED,
    );
    d.both(
        "get_account_with_hash",
        fc.get_account_with_hash(ALICE).await,
        pg.get_account_with_hash(ALICE).await,
        MINTED,
    );
    d.both(
        "effective_permissions",
        fc.effective_permissions(ALICE).await,
        pg.effective_permissions(ALICE).await,
        &[],
    );

    let source = format!("team@{DOMAIN}");
    d.both(
        "upsert_local_alias",
        fc.upsert_local_alias(&source, ALICE).await,
        pg.upsert_local_alias(&source, ALICE).await,
        &[],
    );
    d.both(
        "list_local_aliases",
        fc.list_local_aliases().await,
        pg.list_local_aliases().await,
        MINTED,
    );

    let group = format!("all@{DOMAIN}");
    for kevy in [FASTCORE_KEVY, PG_CORE_KEVY] {
        put_email_group(kevy, &group, &[ALICE, BOB]);
    }
    for address in [
        ALICE,
        source.as_str(),
        group.as_str(),
        &format!("postmaster@{DOMAIN}"),
        &format!("abuse@{DOMAIN}"),
        &format!("SRS0=abcd=TT=remote.test=someone@{DOMAIN}"),
        &format!("nobody@{DOMAIN}"),
        "someone@remote.test",
    ] {
        d.both(
            &format!("recipient_deliverable {address}"),
            fc.recipient_deliverable(address).await,
            pg.recipient_deliverable(address).await,
            &[],
        );
    }
    d.both(
        "delete_local_alias",
        fc.delete_local_alias(&source).await,
        pg.delete_local_alias(&source).await,
        &[],
    );
    d.both(
        "list_local_aliases after delete",
        fc.list_local_aliases().await,
        pg.list_local_aliases().await,
        MINTED,
    );
}

/// Last, so nothing after it needs the account.
pub(super) async fn teardown(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    d.both(
        "remove_account",
        fc.remove_account(BOB).await,
        pg.remove_account(BOB).await,
        &[],
    );
    d.both(
        "list_accounts after remove",
        fc.list_accounts().await,
        pg.list_accounts().await,
        MINTED,
    );
    let other = "other.test";
    d.both(
        "add_domain other",
        fc.add_domain(other).await,
        pg.add_domain(other).await,
        &[],
    );
    d.both(
        "remove_domain",
        fc.remove_domain(other).await,
        pg.remove_domain(other).await,
        &[],
    );
    d.both(
        "list_domains after remove",
        fc.list_domains().await,
        pg.list_domains().await,
        MINTED,
    );
}

/// An email group as the admin surface stores it in the network kevy, which
/// is where both cores read groups from.
fn put_email_group(kevy: &str, address: &str, members: &[&str]) {
    use mailrs_core_sidestate::families::email_groups::{GROUPS_KEY, members_key};
    let mut conn = kevy_client::Connection::connect(kevy).expect("kevy");
    let row = serde_json::json!({ "id": 1, "address": address, "name": "everyone" });
    conn.hset(
        GROUPS_KEY.as_bytes(),
        &[(b"1".as_slice(), row.to_string().as_bytes())],
    )
    .expect("group row");
    let members: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
    conn.sadd(members_key(1).as_bytes(), &members)
        .expect("group members");
}
