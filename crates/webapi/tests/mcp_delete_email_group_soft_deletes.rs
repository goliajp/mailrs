//! The MCP tool `delete_email_group` deletes a team inbox as the REST
//! route does: out of use at once, its working state and reply claims kept
//! for the purge, the delete audited with who did it.

mod live;
mod team_seed;

use mailrs_core_sidestate::families::{email_groups, group_purge};
use reqwest::Method;
use team_seed::*;

const KEVY: &str = "mem://webapi-mcp-group-soft-delete";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_mcp_delete_soft_deletes_like_rest() {
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
    claim_reply(&live, r).await;
    let before = team_keys(&mut net, gid);

    let mut mcp = live.mcp(&admin).await;
    let done = mcp
        .tool("delete_email_group", serde_json::json!({ "id": gid }))
        .await
        .expect("delete_email_group");
    assert_eq!(done, serde_json::json!({ "ok": true }));

    let (_, list) = live
        .call(Method::GET, "/api/admin/email-groups", &admin, None)
        .await;
    assert_eq!(list["items"], serde_json::json!([]), "no longer listed");
    assert_eq!(
        email_groups::find_by_address(&mut net, TEAM).expect("find"),
        None
    );
    let (status, _) = live
        .call(
            Method::GET,
            &format!("/api/shared/{gid}/{}", r.conv),
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 403, "no longer alice's team");
    assert_eq!(
        team_keys(&mut net, gid),
        before,
        "the working state is kept"
    );
    assert_eq!(claim_groups(&mut net), [gid]);
    assert_eq!(reply_seq(&live, r), 1, "the core's reply claim is kept");
    let soft = audit_rows(&mut net, group_purge::SOFT_DELETED);
    assert_eq!(soft.len(), 1, "{soft:?}");
    assert_eq!(soft[0]["actor"], ADMIN);
    assert_eq!(soft[0]["target"], gid.to_string());
    let pass = live.core.group_purge(true, None).await.expect("dry run");
    assert_eq!((pass.pending, pass.due), (1, 0));
}
