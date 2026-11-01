//! The sectioned inbox: each core files the seeded mail into the same
//! sections, and accepting a first contact moves it out of New on both.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::senders::AcceptSendersRequest;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs};

const MINTED: &[&str] = &["id", "mailbox_id", "modseq", "new_modseq", "affected"];

/// The sectioned inbox's views, compared with the rest after every step.
pub(super) fn views(base: &ConversationFilter) -> Vec<(&'static str, ConversationFilter)> {
    vec![
        (
            "section reply",
            ConversationFilter {
                section: Some("reply".into()),
                ..base.clone()
            },
        ),
        (
            "section awaiting",
            ConversationFilter {
                section: Some("awaiting".into()),
                ..base.clone()
            },
        ),
        (
            "section awaiting, due",
            ConversationFilter {
                section: Some("awaiting".into()),
                followup_due_by: Some(1_700_000_650),
                ..base.clone()
            },
        ),
        (
            "section new",
            ConversationFilter {
                section: Some("new".into()),
                ..base.clone()
            },
        ),
        (
            "section feed",
            ConversationFilter {
                section: Some("feed".into()),
                ..base.clone()
            },
        ),
        (
            "section other",
            ConversationFilter {
                section: Some("other".into()),
                ..base.clone()
            },
        ),
    ]
}

fn section(name: &str) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            section: Some(name.into()),
            ..Default::default()
        },
    }
}

async fn compare(c: &Cores, d: &mut Diffs, step: &str) -> Vec<(String, Vec<String>)> {
    let mut seen = Vec::new();
    for name in ["reply", "awaiting", "new", "feed", "other"] {
        let rows = d.both(
            &format!("section {name} {step}"),
            c.fastcore.list_conversations(ALICE, &section(name)).await,
            c.pg_core.list_conversations(ALICE, &section(name)).await,
            MINTED,
        );
        let ids = rows
            .map(|(r, _)| {
                r.items
                    .into_iter()
                    .map(|i| i.thread_id.to_string())
                    .collect()
            })
            .unwrap_or_default();
        seen.push((name.to_string(), ids));
    }
    seen
}

fn has(seen: &[(String, Vec<String>)], section: &str, tid: &str) -> bool {
    seen.iter()
        .any(|(name, ids)| name == section && ids.iter().any(|i| i == tid))
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let before = compare(c, d, "before accepting").await;
    assert!(has(&before, "new", "t-intro@far.test"), "{before:?}");
    assert!(has(&before, "feed", "t-sale@shop.test"), "{before:?}");
    assert!(
        !before
            .iter()
            .any(|(_, ids)| ids.iter().any(|i| i == "t-spam@spam.test")),
        "Junk is in no section: {before:?}"
    );

    let req = AcceptSendersRequest {
        addresses: vec!["EVE@far.test".into(), "eve@far.test".into()],
        accepted_at: 1_700_001_000,
    };
    d.both(
        "accept_senders",
        c.fastcore.accept_senders(ALICE, &req).await,
        c.pg_core.accept_senders(ALICE, &req).await,
        &[],
    );
    let after = compare(c, d, "after accepting").await;
    assert!(!has(&after, "new", "t-intro@far.test"), "{after:?}");
    assert!(has(&after, "reply", "t-intro@far.test"), "{after:?}");
}
