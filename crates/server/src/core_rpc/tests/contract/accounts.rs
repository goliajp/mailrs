//! Accounts, domains, aliases, permissions: the admin surface webapi and the
//! receiver call.

use mailrs_core_api::method::admin::{
    AddAccountRequest, SendAsList, SetPasswordRequest, SetQuotaRequest, UpdateAccountRequest,
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
            password_hash: None,
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
    send_as(c, d, &source, &group).await;
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
    created_with_a_hash(c, d).await;
    cleared(c, d).await;
}

/// Clearing empties the hash and records who did it; setting a password
/// afterwards removes the record. An unknown account is 404 on both.
async fn cleared(c: &Cores, d: &mut Diffs) {
    use mailrs_core_api::method::admin::ClearPasswordRequest;
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let req = ClearPasswordRequest {
        by: "root@example.test".into(),
    };
    d.both(
        "clear_account_password",
        fc.clear_account_password(BOB, &req).await,
        pg.clear_account_password(BOB, &req).await,
        &[],
    );
    if let Some((a, b)) = d.both(
        "get_account_with_hash after clear",
        fc.get_account_with_hash(BOB).await,
        pg.get_account_with_hash(BOB).await,
        &["password_cleared_at", "created_at"],
    ) {
        for (core, got) in [("fastcore", a), ("pg-core", b)] {
            let ok = got.password_hash.as_deref() == Some("")
                && got.public.password_cleared_at.is_some()
                && got.public.password_cleared_by.as_deref() == Some("root@example.test");
            if !ok {
                d.0.push(format!("{core} after clear: {:?}", got.public));
            }
        }
    }
    d.both(
        "clear_account_password unknown",
        fc.clear_account_password(&format!("nobody@{DOMAIN}"), &req)
            .await,
        pg.clear_account_password(&format!("nobody@{DOMAIN}"), &req)
            .await,
        &[],
    );
    let hash = mailrs_core::users::UserStore::hash_password("set-again").expect("hash");
    let set = SetPasswordRequest {
        password_hash: hash.clone(),
    };
    d.both(
        "set_account_password after clear",
        fc.set_account_password(BOB, &set).await,
        pg.set_account_password(BOB, &set).await,
        &[],
    );
    if let Some((a, b)) = d.both(
        "get_account_with_hash after a new password",
        fc.get_account_with_hash(BOB).await,
        pg.get_account_with_hash(BOB).await,
        MINTED,
    ) {
        for (core, got) in [("fastcore", a), ("pg-core", b)] {
            let ok = got.password_hash.as_deref() == Some(hash.as_str())
                && got.public.password_cleared_at.is_none()
                && got.public.password_cleared_by.is_none();
            if !ok {
                d.0.push(format!("{core} after a new password: {:?}", got.public));
            }
        }
    }
}

/// An account made with a ready hash keeps exactly that hash, argon2 or
/// empty; a hash that is not argon2, or no password at all, is refused.
async fn created_with_a_hash(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let argon2 = mailrs_core::users::UserStore::hash_password("kept-pw").expect("hash");
    for (address, hash) in [
        (format!("kept-hash@{DOMAIN}"), argon2.as_str()),
        (format!("no-password@{DOMAIN}"), ""),
    ] {
        let req = AddAccountRequest {
            address: address.clone(),
            display_name: String::new(),
            password: String::new(),
            password_hash: Some(hash.into()),
        };
        d.both(
            "add_account with a hash",
            fc.add_account(&req).await,
            pg.add_account(&req).await,
            &[],
        );
        if let Some((a, b)) = d.both(
            "get_account_with_hash after a create with one",
            fc.get_account_with_hash(&address).await,
            pg.get_account_with_hash(&address).await,
            MINTED,
        ) {
            for (core, got) in [("fastcore", a), ("pg-core", b)] {
                let stored = got.password_hash.unwrap_or_default();
                if stored != hash {
                    d.0.push(format!(
                        "{core} stored {stored:?} for {address}, given {hash:?}"
                    ));
                }
            }
        }
    }
    for (address, password, hash) in [
        (format!("clear-text@{DOMAIN}"), "", Some("clear-text-1")),
        (format!("nothing@{DOMAIN}"), "", None),
    ] {
        let req = AddAccountRequest {
            address: address.clone(),
            display_name: String::new(),
            password: password.into(),
            password_hash: hash.map(str::to_string),
        };
        let (a, b) = (fc.add_account(&req).await, pg.add_account(&req).await);
        if a.is_ok() || b.is_ok() {
            d.0.push(format!(
                "add_account {address} accepted: fastcore {:?}, pg-core {:?}",
                a.is_ok(),
                b.is_ok()
            ));
        }
        d.both("add_account refused", a, b, &[]);
    }
}

/// What ALICE may send as, with `alias` pointing at her and `group` holding
/// her: both cores must count the alias and the group, keep a catch-all
/// out, and add what an administrator granted.
async fn send_as(c: &Cores, d: &mut Diffs, alias: &str, group: &str) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let catch_all = format!("@{DOMAIN}");
    d.both(
        "upsert_local_alias catch-all",
        fc.upsert_local_alias(&catch_all, ALICE).await,
        pg.upsert_local_alias(&catch_all, ALICE).await,
        &[],
    );
    let derived = fc
        .effective_permissions(ALICE)
        .await
        .expect("fastcore perms");
    assert_eq!(
        derived.send_as,
        vec![group.to_string(), alias.to_string()],
        "an alias and a group, never the catch-all"
    );
    d.both(
        "effective_permissions with alias and group",
        fc.effective_permissions(ALICE).await,
        pg.effective_permissions(ALICE).await,
        &[],
    );
    let grant = SendAsList {
        send_as: vec![format!(" noreply@{DOMAIN} "), format!("noreply@{DOMAIN}")],
    };
    for (label, address) in [
        ("set_send_as", ALICE),
        ("set_send_as unknown", "ghost@nowhere.test"),
    ] {
        d.both(
            label,
            fc.set_send_as(address, &grant).await,
            pg.set_send_as(address, &grant).await,
            &[],
        );
    }
    let bad = SendAsList {
        send_as: vec!["not-an-address".into()],
    };
    d.both(
        "set_send_as not an address",
        fc.set_send_as(ALICE, &bad).await,
        pg.set_send_as(ALICE, &bad).await,
        &[],
    );
    d.both(
        "get_send_as",
        fc.get_send_as(ALICE).await,
        pg.get_send_as(ALICE).await,
        &[],
    );
    assert_eq!(
        fc.get_send_as(ALICE).await.expect("grants").send_as,
        vec![format!("noreply@{DOMAIN}")]
    );
    d.both(
        "effective_permissions with a grant",
        fc.effective_permissions(ALICE).await,
        pg.effective_permissions(ALICE).await,
        &[],
    );
    d.both(
        "set_send_as cleared",
        fc.set_send_as(ALICE, &SendAsList::default()).await,
        pg.set_send_as(ALICE, &SendAsList::default()).await,
        &[],
    );
    d.both(
        "delete_local_alias catch-all",
        fc.delete_local_alias(&catch_all).await,
        pg.delete_local_alias(&catch_all).await,
        &[],
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
