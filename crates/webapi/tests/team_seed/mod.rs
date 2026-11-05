//! A team inbox created through the API, with working state in every
//! family a team writes, for the tests of deleting one.

// each test binary that includes this uses a different subset of it
#![allow(dead_code)]

use mailrs_core_sidestate::families::shared_inbox::{
    self, ConvRef, HeldClaim, assign, drafts, notes, presence,
};
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;
use reqwest::Method;

use super::live::Live;

pub const ADMIN: &str = "admin@example.test";
pub const ALICE: &str = "alice@example.test";
pub const PASSWORD: &str = "pw";
pub const TEAM: &str = "support@example.test";
pub const ORDER_ID: &str = "<order1@oldmail.net>";

/// The administrator, signed in and confirmed, and alice, signed in.
pub async fn people(live: &Live) -> (String, String) {
    live.add_account(ADMIN, PASSWORD).await;
    live.add_account(ALICE, PASSWORD).await;
    live.make_super(ADMIN);
    let admin = live.login(ADMIN, PASSWORD).await;
    live.confirm(&admin, PASSWORD).await;
    let alice = live.login(ALICE, PASSWORD).await;
    (admin, alice)
}

/// Create the team inbox `TEAM` with alice in it; its id.
pub async fn create_team(live: &Live, admin: &str) -> i64 {
    let (status, g) = live
        .call(
            Method::POST,
            "/api/admin/email-groups",
            admin,
            Some(serde_json::json!({
                "address": TEAM, "domain": "example.test", "name": "Support",
                "members": [ALICE],
            })),
        )
        .await;
    assert_eq!(status, 200, "{g}");
    let gid = g["id"].as_i64().expect("id");
    let (status, body) = live
        .call(
            Method::PUT,
            &format!("/api/admin/email-groups/{gid}/shared"),
            admin,
            Some(serde_json::json!({ "shared": true })),
        )
        .await;
    assert_eq!(status, 204, "{body}");
    gid
}

pub async fn delete_team(live: &Live, admin: &str, gid: i64) {
    let (status, body) = live
        .call(
            Method::DELETE,
            &format!("/api/admin/email-groups/{gid}"),
            admin,
            None,
        )
        .await;
    assert_eq!(status, 204, "delete: {body}");
}

/// One team conversation of `gid` with a copy, an assignment, a note, a
/// draft, presence and a reply claim held by alice; its reference.
pub fn work(net: &mut kevy_client::Connection, gid: i64) -> ConvRef {
    let r = shared_inbox::conv_for_arrival(net, gid, ORDER_ID, "", &[]).expect("conv");
    shared_inbox::record_copy(net, r, ALICE, "t-alice").expect("copy");
    let fact = assign::AssignFact {
        at: 10,
        by: ALICE.into(),
        to: Some(ALICE.into()),
    };
    assign::append(net, r, &fact).expect("assign");
    notes::add(net, r, ALICE, "call the customer back", 11).expect("note");
    drafts::announce(net, r, ALICE, 7, 12)
        .expect("draft")
        .expect("announced");
    presence::heartbeat(net, r, ALICE, presence::State::Viewing).expect("presence");
    let claim = HeldClaim {
        gid,
        conv: r.conv,
        claim_id: format!("claim-{gid}"),
    };
    shared_inbox::hold_claim(net, ALICE, &format!("<reply-{gid}@example.test>"), &claim)
        .expect("claim");
    r
}

/// Alice claims the reply to `r` through the core: the reply claim the
/// core keeps in its own store.
pub async fn claim_reply(live: &Live, r: ConvRef) {
    let req = mailrs_core_api::method::shared::ClaimReplyRequest {
        gid: r.gid,
        conv: r.conv,
        seen_seq: 0,
        force: false,
        at: 20,
    };
    live.core
        .shared_claim_reply(ALICE, &req)
        .await
        .expect("claim the reply");
    assert_eq!(reply_seq(live, r), 1, "the claim is in the core's store");
}

/// How many replies the core's store says were claimed on `r`.
pub fn reply_seq(live: &Live, r: ConvRef) -> u64 {
    live.mailbox.reply_state(&r.tag()).expect("reply state").seq
}

/// The keys of group `gid`'s working state, sorted.
pub fn team_keys(net: &mut kevy_client::Connection, gid: i64) -> Vec<String> {
    let mut keys: Vec<String> = net
        .keys(shared_inbox::group_glob(gid).as_bytes())
        .expect("keys")
        .iter()
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect();
    keys.sort();
    keys
}

/// The groups alice's held reply claims belong to, sorted.
pub fn claim_groups(net: &mut kevy_client::Connection) -> Vec<i64> {
    let flat = net
        .hgetall(shared_inbox::claims_key(ALICE).as_bytes())
        .expect("claims");
    let mut out: Vec<i64> = flat
        .chunks(2)
        .filter_map(|kv| serde_json::from_slice::<HeldClaim>(kv.get(1)?).ok())
        .map(|c| c.gid)
        .collect();
    out.sort();
    out
}

/// The audit rows with `action`, newest first.
pub fn audit_rows(net: &mut kevy_client::Connection, action: &str) -> Vec<serde_json::Value> {
    net.lrange(AUDIT_LOG, 0, -1)
        .expect("audit")
        .iter()
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(r).ok())
        .filter(|r| r["action"] == action)
        .collect()
}
