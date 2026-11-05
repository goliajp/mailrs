//! A deleted account's data waits out its retention period: the delete
//! parks the maildir and marks the address, a purge before the period is
//! up finds nothing due, a dry run counts every store and writes nothing,
//! the address cannot be taken while it waits, and once the period is over
//! the purge removes it all, writes one audit row, and a second pass has
//! nothing to do.

mod purge_support;

use mailrs_core_api::method::admin::{AccountPurgeResponse, PATH_ACCOUNT_PURGE};
use mailrs_core_sidestate::families::account_purge::{PURGED, pending};
use mailrs_core_sidestate::families::audit::AuditFact;
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;
use purge_support::*;

fn set_days(days: &str) {
    // SAFETY: the only test in this binary; nothing else reads it meanwhile
    unsafe { std::env::set_var(pending::RETENTION_ENV, days) };
}

async fn purge(f: &Fixture, query: &str) -> AccountPurgeResponse {
    let (status, body) = call(
        &f.state,
        "POST",
        &format!("{PATH_ACCOUNT_PURGE}{query}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    serde_json::from_value(body).unwrap()
}

fn purges(net: &mut kevy_client::Connection) -> Vec<AuditFact> {
    net.lrange(AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice::<AuditFact>(r).ok())
        .filter(|a| a.action == PURGED)
        .collect()
}

#[tokio::test]
async fn the_purge_waits_for_the_retention_period_and_then_takes_everything() {
    set_days("30");
    let f = fixture().await;
    let mut mail = mail_conn(&f.state);
    let mut net = kevy_client::Connection::connect(&f.kevy_url).unwrap();

    let (status, _) = call(
        &f.state,
        "DELETE",
        &format!("/v1/admin/accounts/{BOB}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_eq!(parked(&f.root).len(), 1, "the maildir is parked");
    assert!(pending::deleted_at(&mut net, BOB).unwrap().is_some());
    assert!(
        !names(&snapshot(&mut mail, "mailrs:threaduser:bob@x.com:*")).is_empty(),
        "the delete purged before the retention period"
    );

    // the address waits: it is not handed to a new account
    let add = serde_json::json!({ "address": BOB, "display_name": "Bob", "password": PASSWORD });
    let (status, _) = call(&f.state, "POST", "/v1/admin/accounts", Some(add.clone())).await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);

    let early = purge(&f, "?dry_run=false").await;
    assert_eq!((early.pending, early.due), (1, 0), "{early:?}");
    assert!(early.rows.is_empty());

    // the period is over: a dry run counts each store and writes nothing
    set_days("0");
    let before_mail = snapshot(&mut mail, "*");
    let before_net = snapshot(&mut net, "*");
    let dry = purge(&f, "").await;
    assert!(dry.dry_run);
    assert_eq!((dry.pending, dry.due, dry.rows.len()), (1, 1, 1), "{dry:?}");
    let row = &dry.rows[0];
    assert!(row.maildir_files >= 3 && row.maildir_bytes > 0, "{row:?}");
    let mail_names: Vec<&str> = row.mail_store.iter().map(|p| p.name.as_str()).collect();
    for family in [
        "mailrs:threaduser:{user}:*",
        "mailrs:usermsg:{user}:*",
        "mailrs:user:{user}:*",
    ] {
        assert!(
            mail_names.contains(&family),
            "{family} not counted: {mail_names:?}"
        );
    }
    let net_names: Vec<&str> = row.network.iter().map(|p| p.name.as_str()).collect();
    for family in [
        mailrs_core_sidestate::sieve_key("{user}"),
        mailrs_core_sidestate::families::prefs::drafts_key("{user}"),
        mailrs_core_sidestate::families::permission_groups::members_key("*"),
    ] {
        assert!(
            net_names.contains(&family.as_str()),
            "{family} not counted: {net_names:?}"
        );
    }
    assert!(row.unregistered.is_empty(), "{:?}", row.unregistered);
    assert_eq!(
        snapshot(&mut mail, "*"),
        before_mail,
        "the dry run wrote to the mail store"
    );
    assert_eq!(
        snapshot(&mut net, "*"),
        before_net,
        "the dry run wrote to the shared store"
    );
    assert_eq!(parked(&f.root).len(), 1);

    let done = purge(&f, "?dry_run=false").await;
    assert_eq!(done.rows.len(), 1, "{done:?}");
    assert_eq!(
        done.rows[0].mail_store, row.mail_store,
        "a purge removes what its dry run counted"
    );
    assert!(
        done.rows[0].unregistered.is_empty(),
        "{:?}",
        done.rows[0].unregistered
    );
    assert!(names(&snapshot(&mut mail, "*bob@x.com*")).is_empty());
    assert!(names(&snapshot(&mut net, "*bob@x.com*")).is_empty());
    assert!(parked(&f.root).is_empty());
    let audited = purges(&mut net);
    assert_eq!(audited.len(), 1);
    assert_eq!(audited[0].target, BOB);
    assert!(
        audited[0].detail.contains("maildir_files"),
        "{}",
        audited[0].detail
    );

    // nothing is due any more: a second pass reads and writes nothing
    let quiet_mail = snapshot(&mut mail, "*");
    let quiet_net = snapshot(&mut net, "*");
    let again = purge(&f, "?dry_run=false").await;
    assert_eq!((again.pending, again.due), (0, 0), "{again:?}");
    assert_eq!(snapshot(&mut mail, "*"), quiet_mail);
    assert_eq!(snapshot(&mut net, "*"), quiet_net);
    assert_eq!(purges(&mut net).len(), 1);

    // and the address is free again, for an account that starts empty
    let (status, _) = call(&f.state, "POST", "/v1/admin/accounts", Some(add)).await;
    assert!(status.is_success(), "{status}");
    assert!(
        f.state
            .mailbox
            .get_thread_for_user(BOB, &f.shared_tid)
            .unwrap()
            .is_none()
    );
}
