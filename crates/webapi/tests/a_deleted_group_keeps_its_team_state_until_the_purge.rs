//! Under the default retention, deleting a team inbox takes it out of use
//! at once and keeps its working state: the group is no longer listed,
//! mail to its address finds no group, its members are no longer its team,
//! and every `shared:{gid}:*` key and held claim is still there. The core's
//! purge finds it pending and not due, and a dry run writes nothing. A
//! group created again on the same address and name gets a new id and
//! reads none of the old state.

mod live;
mod team_seed;

use mailrs_core_sidestate::families::{email_groups, group_purge, shared_inbox};
use reqwest::Method;
use team_seed::*;

const KEVY: &str = "mem://webapi-group-soft-delete";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_team_is_gone_at_once_and_its_state_waits_for_the_purge() {
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_DELETED_ACCOUNT_DAYS", "30");
    }
    let live = live::start(KEVY).await;
    let (admin, alice) = people(&live).await;
    let gid = create_team(&live, &admin).await;
    let mut net = kevy_client::Connection::connect(KEVY).expect("kevy");
    let r = work(&mut net, gid);
    let page = format!("/api/shared/{gid}/{}", r.conv);
    let (status, _) = live.call(Method::GET, &page, &alice, None).await;
    assert_eq!(status, 200, "alice reads her team before the delete");
    let before = team_keys(&mut net, gid);
    assert!(before.len() >= 7, "{before:?}");

    delete_team(&live, &admin, gid).await;

    // out of use at once
    let (_, list) = live
        .call(Method::GET, "/api/admin/email-groups", &admin, None)
        .await;
    assert_eq!(list["items"], serde_json::json!([]), "no longer listed");
    assert_eq!(
        email_groups::find_by_address(&mut net, TEAM).expect("find"),
        None,
        "mail to the address finds no group"
    );
    for path in [
        page.clone(),
        format!("{page}/notes"),
        format!("{page}/draft"),
    ] {
        let (status, _) = live.call(Method::GET, &path, &alice, None).await;
        assert_eq!(status, 403, "{path}: no longer alice's team");
    }
    let (status, _) = live
        .call(
            Method::POST,
            &format!("{page}/assign"),
            &alice,
            Some(serde_json::json!({ "to": ALICE })),
        )
        .await;
    assert_ne!(status, 200, "no longer assignable");
    let soft = audit_rows(&mut net, group_purge::SOFT_DELETED);
    assert_eq!(soft.len(), 1, "{soft:?}");
    assert_eq!(soft[0]["actor"], ADMIN);

    // its state kept, pending and not due
    assert_eq!(team_keys(&mut net, gid), before);
    assert_eq!(claim_groups(&mut net), [gid]);
    let pass = live.core.group_purge(false, Some(gid)).await.expect("pass");
    assert_eq!((pass.retention_days, pass.pending, pass.due), (30, 1, 0));
    assert!(pass.rows.is_empty());
    let dry = live.core.group_purge(true, None).await.expect("dry run");
    assert_eq!((dry.dry_run, dry.due), (true, 0));
    assert_eq!(team_keys(&mut net, gid), before);
    assert!(audit_rows(&mut net, group_purge::PURGED).is_empty());

    // the same group again: a new id, none of the old state
    let again = create_team(&live, &admin).await;
    assert_ne!(again, gid, "a group id is never handed out twice");
    let (status, overview) = live
        .call(
            Method::GET,
            &format!("/api/shared/{again}/{}", r.conv),
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 200, "{overview}");
    assert_eq!(overview["assignee"], serde_json::Value::Null, "{overview}");
    assert_eq!(overview["assignments"], serde_json::json!([]), "{overview}");
    assert_eq!(overview["draft"], serde_json::Value::Null, "{overview}");
    assert_eq!(overview["present"], serde_json::json!([]), "{overview}");
    let (status, notes) = live
        .call(
            Method::GET,
            &format!("/api/shared/{again}/{}/notes", r.conv),
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 200, "{notes}");
    assert_eq!(notes, serde_json::json!([]), "{notes}");
    // the order it received before starts a conversation of its own
    let fresh = shared_inbox::conv_for_arrival(&mut net, again, ORDER_ID, "", &[]).expect("conv");
    assert_eq!(fresh.conv, 1);
    assert_eq!(
        shared_inbox::copies(&mut net, fresh).expect("copies"),
        Vec::<(String, String)>::new()
    );
    assert_eq!(
        team_keys(&mut net, gid),
        before,
        "the old state is still the old group's"
    );
}
