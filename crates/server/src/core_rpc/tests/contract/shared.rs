//! Team inboxes: a group's mail reaches each member tied to one team
//! conversation, assignment shows up on every member's copy and in
//! "assigned to me", and of two replies claimed on the same count only one
//! wins, on both cores.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::shared::{
    AssignRequest, ClaimReplyRequest, ClaimReplyResponse, ReleaseReplyRequest, StateRequest,
};
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};

use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, DOMAIN};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];
pub(super) const GID: i64 = 2;

pub(super) const ORDER: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: support@example.test\r\n\
Subject: Where is my order\r\n\
Message-ID: <order1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
It has not arrived.\r\n";

pub(super) fn team_group(kevy: &str) {
    use mailrs_core_sidestate::families::email_groups::{GROUPS_KEY, members_key, set_shared};
    let mut conn = kevy_client::Connection::connect(kevy).expect("kevy");
    let row =
        serde_json::json!({ "id": GID, "address": format!("support@{DOMAIN}"), "name": "support" });
    conn.hset(
        GROUPS_KEY.as_bytes(),
        &[(GID.to_string().as_bytes(), row.to_string().as_bytes())],
    )
    .expect("group row");
    conn.sadd(
        members_key(GID).as_bytes(),
        &[ALICE.as_bytes(), BOB.as_bytes()],
    )
    .expect("members");
    assert!(set_shared(&mut conn, GID, true).expect("shared"));
}

async fn rows(
    c: &Cores,
    d: &mut Diffs,
    user: &str,
    assigned: bool,
) -> Vec<ConversationSummaryWire> {
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            assigned_to_me: assigned,
            ..Default::default()
        },
    };
    d.both(
        &format!("list_conversations {user} assigned_to_me={assigned}"),
        c.fastcore.list_conversations(user, &req).await,
        c.pg_core.list_conversations(user, &req).await,
        MINTED,
    )
    .map(|(_, pg)| pg.items)
    .unwrap_or_default()
}

/// (actor, action, target, detail) of one core's `shared.*` audit rows,
/// oldest first.
fn assign_audit(kevy: &str) -> Vec<(String, String, String, String)> {
    use mailrs_core_sidestate::families::audit::AuditFact;
    let mut c = kevy_client::Connection::connect(kevy).expect("kevy");
    let mut got: Vec<_> = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit")
        .iter()
        .map(|r| serde_json::from_slice::<AuditFact>(r).expect("fact"))
        .filter(|f| f.action.starts_with("shared."))
        .map(|f| (f.actor, f.action, f.target, f.detail))
        .collect();
    got.reverse();
    got
}

fn claim(seen_seq: u64, force: bool, at: i64) -> ClaimReplyRequest {
    ClaimReplyRequest {
        gid: GID,
        conv: 1,
        seen_seq,
        force,
        at,
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    for kevy in [FASTCORE_KEVY, PG_CORE_KEVY] {
        team_group(kevy);
    }
    let spools = Spools::new(c);
    let mut env = envelope(None);
    env.forward_paths = vec![format!("support@{DOMAIN}")];
    spools.arrive(c, &env, ORDER).await;

    for user in [ALICE, BOB] {
        let listed = rows(c, d, user, false).await;
        let copy = listed
            .iter()
            .find(|r| r.thread_id == "order1@outside.example")
            .unwrap_or_else(|| panic!("{user} has no copy"));
        assert_eq!(
            copy.shared_conv, "2:1",
            "{user}'s copy is not tied to the team"
        );
        assert_eq!(copy.assignee, "");
    }

    let assign = AssignRequest {
        gid: GID,
        conv: 1,
        to: Some(BOB.into()),
        at: 1_790_000_000,
    };
    let done = d.both(
        "shared_assign",
        c.fastcore.shared_assign(ALICE, &assign).await,
        c.pg_core.shared_assign(ALICE, &assign).await,
        &[],
    );
    assert_eq!(done.map(|(_, pg)| pg.rows), Some(2));
    let mine = rows(c, d, BOB, true).await;
    assert_eq!(mine.len(), 1, "bob's assigned list: {mine:?}");
    assert_eq!(mine[0].assignee, BOB);
    assert!(rows(c, d, ALICE, true).await.is_empty());

    let outsider = AssignRequest {
        to: None,
        ..assign.clone()
    };
    d.both(
        "shared_assign by a non-member",
        c.fastcore
            .shared_assign("carol@example.test", &outsider)
            .await,
        c.pg_core
            .shared_assign("carol@example.test", &outsider)
            .await,
        &[],
    );

    let back = AssignRequest {
        to: None,
        at: 1_790_000_100,
        ..assign.clone()
    };
    d.both(
        "shared_assign taken back",
        c.fastcore.shared_assign(BOB, &back).await,
        c.pg_core.shared_assign(BOB, &back).await,
        &[],
    );
    let read = StateRequest { gid: GID, conv: 1 };
    d.both(
        "shared_state before any reply",
        c.fastcore.shared_state(ALICE, &read).await,
        c.pg_core.shared_state(ALICE, &read).await,
        &[],
    );
    // each core audits every change, and neither the refusal nor the read
    let line = |actor: &str, action: &str, detail: &str| {
        (
            actor.to_string(),
            action.to_string(),
            "2:1".to_string(),
            detail.to_string(),
        )
    };
    let want = vec![
        line(ALICE, "shared.assign", BOB),
        line(BOB, "shared.unassign", ""),
    ];
    assert_eq!(assign_audit(FASTCORE_KEVY), want, "fastcore");
    assert_eq!(assign_audit(PG_CORE_KEVY), want, "SQL core");

    // two members answer the same message: one claim wins
    let a = d.both(
        "shared_claim_reply alice",
        c.fastcore
            .shared_claim_reply(ALICE, &claim(0, false, 10))
            .await,
        c.pg_core
            .shared_claim_reply(ALICE, &claim(0, false, 10))
            .await,
        &[],
    );
    let b = d.both(
        "shared_claim_reply bob on the same count",
        c.fastcore
            .shared_claim_reply(BOB, &claim(0, false, 11))
            .await,
        c.pg_core
            .shared_claim_reply(BOB, &claim(0, false, 11))
            .await,
        &[],
    );
    assert!(
        matches!(a, Some((ClaimReplyResponse::Claimed { .. }, _))),
        "{a:?}"
    );
    assert!(
        matches!(b, Some((ClaimReplyResponse::Conflict { .. }, _))),
        "{b:?}"
    );
    let forced = d.both(
        "shared_claim_reply bob forced",
        c.fastcore
            .shared_claim_reply(BOB, &claim(0, true, 12))
            .await,
        c.pg_core.shared_claim_reply(BOB, &claim(0, true, 12)).await,
        &[],
    );
    let Some((ClaimReplyResponse::Claimed { claim_id, .. }, _)) = forced else {
        panic!("a forced claim lost: {forced:?}");
    };
    let state = StateRequest { gid: GID, conv: 1 };
    d.both(
        "shared_state",
        c.fastcore.shared_state(ALICE, &state).await,
        c.pg_core.shared_state(ALICE, &state).await,
        &[],
    );
    let release = ReleaseReplyRequest {
        gid: GID,
        conv: 1,
        claim_id,
    };
    let released = d.both(
        "shared_release_reply",
        c.fastcore.shared_release_reply(BOB, &release).await,
        c.pg_core.shared_release_reply(BOB, &release).await,
        &[],
    );
    assert_eq!(released.map(|(_, pg)| pg.released), Some(true));
    let again = d.both(
        "shared_state after the undo",
        c.fastcore.shared_state(ALICE, &state).await,
        c.pg_core.shared_state(ALICE, &state).await,
        &[],
    );
    assert_eq!(again.map(|(_, pg)| pg.reply.seq), Some(1));
}
