//! A search finds what the list it was typed into shows, on both cores:
//! the same filter scopes the list and the search inside it, for every
//! list a client has, and each reader's own copy decides.

use mailrs_core_api::method::conversation::{ListConversationsRequest, SearchConversationsRequest};
use mailrs_core_api::method::label::CreateLabelRequest;
use mailrs_core_api::method::shared::AssignRequest;
use mailrs_core_api::method::thread::SnoozeRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{PHISH, Spools, envelope, held};
use super::shared::{GID, ORDER, team_group};
use super::{ALICE, BOB, DOMAIN, accounts, mail};

/// Ids each store mints, and `trashed_at`, which each core stamps from its
/// own clock, so the two can straddle a second.
const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq", "trashed_at"];

const FC_KEVY: &str = "mem://search-scope-fastcore-side-state";
const PG_KEVY: &str = "mem://search-scope-pg-core-side-state";

pub(super) const HELLO: &str = "t-hello@partner.test";
pub(super) const INVOICE: &str = "t-invoice@billing.test";
pub(super) const SALE: &str = "t-sale@shop.test";
const SENT: &str = "t-sent@example.test";
pub(super) const SHARED: &str = "t-shared@example.test";
pub(super) const INTRO: &str = "t-intro@far.test";
pub(super) const PHISHED: &str = "p1@outside.example";
pub(super) const TEAM: &str = "order1@outside.example";

/// Matches every conversation, so what comes back is the scope alone.
const EVERYTHING: &str = "after:2000-01-01";

fn filter(f: ConversationFilter) -> ConversationFilter {
    ConversationFilter { limit: 50, ..f }
}

/// The lists a client searches from, by the filter it lists them with.
pub(super) fn lists() -> Vec<(&'static str, ConversationFilter)> {
    let none = ConversationFilter::default();
    vec![
        ("no list", filter(none.clone())),
        (
            "inbox",
            filter(ConversationFilter {
                folder: Some("Inbox".into()),
                ..none.clone()
            }),
        ),
        (
            "inbox, held shown in place",
            filter(ConversationFilter {
                folder: Some("Inbox".into()),
                include_quarantined: true,
                ..none.clone()
            }),
        ),
        (
            "all mail",
            filter(ConversationFilter {
                folder: Some("all".into()),
                ..none.clone()
            }),
        ),
        (
            "archived",
            filter(ConversationFilter {
                archived: true,
                ..none.clone()
            }),
        ),
        (
            "pinned",
            filter(ConversationFilter {
                pinned: Some(true),
                ..none.clone()
            }),
        ),
        (
            "snoozed",
            filter(ConversationFilter {
                snoozed: Some(true),
                ..none.clone()
            }),
        ),
        (
            "waiting",
            filter(ConversationFilter {
                section: Some("awaiting".into()),
                ..none.clone()
            }),
        ),
        (
            "assigned to me",
            filter(ConversationFilter {
                assigned_to_me: true,
                ..none.clone()
            }),
        ),
        (
            "trash",
            filter(ConversationFilter {
                trashed: true,
                ..none.clone()
            }),
        ),
        (
            "label",
            filter(ConversationFilter {
                label: Some("1".into()),
                ..none.clone()
            }),
        ),
    ]
}

fn sorted(items: Vec<mailrs_core_api::types::ConversationSummaryWire>) -> Vec<String> {
    let mut ids: Vec<String> = items.into_iter().map(|i| i.thread_id).collect();
    ids.sort();
    ids
}

/// What a search from `f` finds on each core, compared, ids sorted.
async fn searched(
    c: &Cores,
    d: &mut Diffs,
    user: &str,
    name: &str,
    req: &SearchConversationsRequest,
) -> [Vec<String>; 2] {
    let by_thread = |r: mailrs_core_api::error::ApiResult<
        mailrs_core_api::method::conversation::SearchConversationsResponse,
    >| {
        r.map(|mut r| {
            r.items.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
            r
        })
    };
    let fc = by_thread(c.fastcore.search_conversations(user, req).await);
    let pg = by_thread(c.pg_core.search_conversations(user, req).await);
    let both = d.both(
        &format!(
            "search_conversations {:?} from [{name}] as {user}",
            req.query
        ),
        fc,
        pg,
        MINTED,
    );
    let (fc, pg) = both.expect("both cores search");
    [sorted(fc.items), sorted(pg.items)]
}

/// The conversations list `f` shows on each core, ids sorted.
async fn listed(c: &Cores, user: &str, f: &ConversationFilter) -> [Vec<String>; 2] {
    let req = ListConversationsRequest { filter: f.clone() };
    [
        sorted(
            c.fastcore
                .list_conversations(user, &req)
                .await
                .expect("list")
                .items,
        ),
        sorted(
            c.pg_core
                .list_conversations(user, &req)
                .await
                .expect("list")
                .items,
        ),
    ]
}

/// Pinned, snoozed, trashed, labelled, assigned and held, on both cores;
/// `kevies` are the side-state stores the cores were started on.
pub(super) async fn arrange(c: &Cores, kevies: [&str; 2]) {
    for kevy in kevies {
        team_group(kevy);
    }
    let spools = Spools::new(c);
    let mut team = envelope(None);
    team.forward_paths = vec![format!("support@{DOMAIN}")];
    spools.arrive(c, &team, ORDER).await;
    spools.arrive(c, &envelope(held()), PHISH).await;

    let label = CreateLabelRequest {
        name: "Work".into(),
        color: "blue".into(),
    };
    let snooze = SnoozeRequest {
        snoozed_until: 4_000_000_000,
    };
    let assign = AssignRequest {
        gid: GID,
        conv: 1,
        to: Some(BOB.into()),
        at: 1_790_000_000,
    };
    for core in [&c.fastcore, &c.pg_core] {
        core.pin_thread(ALICE, HELLO).await.expect("pin");
        // Bob's copy of the thread they share, not Alice's
        core.pin_thread(BOB, SHARED).await.expect("pin");
        core.snooze_thread(ALICE, INVOICE, &snooze)
            .await
            .expect("snooze");
        core.trash_thread(ALICE, SALE).await.expect("trash");
        core.create_label(ALICE, &label).await.expect("label");
        core.set_thread_label(ALICE, INTRO, "1", true)
            .await
            .expect("label");
        core.shared_assign(ALICE, &assign).await.expect("assign");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_search_finds_what_its_list_shows_on_both_cores() {
    let cores = harness::start_on(FC_KEVY, PG_KEVY).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;
    arrange(&cores, [FC_KEVY, PG_KEVY]).await;

    // what each list must hold, so two empty answers cannot agree
    let expect: &[(&str, &str, &[&str])] = &[
        (ALICE, "pinned", &[HELLO]),
        (BOB, "pinned", &[SHARED]),
        (ALICE, "snoozed", &[INVOICE]),
        (ALICE, "waiting", &[SENT]),
        (BOB, "assigned to me", &[TEAM]),
        (ALICE, "assigned to me", &[]),
        (ALICE, "trash", &[SALE]),
        (ALICE, "label", &[INTRO]),
    ];
    // every list checked before failing, so one run names every scope
    let mut wrong = Vec::new();
    for user in [ALICE, BOB] {
        for (name, f) in lists() {
            let want = listed(&cores, user, &f).await;
            let req = SearchConversationsRequest {
                query: EVERYTHING.into(),
                filter: f.clone(),
                quarantined: false,
            };
            let got = searched(&cores, &mut d, user, name, &req).await;
            for (core, (got, want)) in ["fastcore", "pg"].iter().zip(got.iter().zip(want.iter())) {
                if got != want {
                    wrong.push(format!(
                        "{core}: search from [{name}] as {user} found {got:?}, the list shows {want:?}"
                    ));
                }
            }
            if let Some((_, _, ids)) = expect.iter().find(|(u, n, _)| *u == user && *n == name) {
                assert_eq!(want[0], *ids, "fastcore: [{name}] as {user}");
                assert_eq!(want[1], *ids, "pg: [{name}] as {user}");
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // words, not only operators: the reader's own pin decides
    for (user, want) in [(ALICE, HELLO), (BOB, SHARED)] {
        let req = SearchConversationsRequest {
            query: "partner.test".into(),
            filter: filter(ConversationFilter {
                pinned: Some(true),
                ..Default::default()
            }),
            quarantined: false,
        };
        let got = searched(&cores, &mut d, user, "pinned", &req).await;
        assert_eq!(
            got,
            [vec![want.to_string()], vec![want.to_string()]],
            "{user}"
        );
    }

    // a held conversation is found from Review, and only from there
    let review = SearchConversationsRequest {
        query: EVERYTHING.into(),
        filter: filter(ConversationFilter::default()),
        quarantined: true,
    };
    let got = searched(&cores, &mut d, ALICE, "review", &review).await;
    assert_eq!(got, [vec![PHISHED.to_string()], vec![PHISHED.to_string()]]);
    let in_place = searched(
        &cores,
        &mut d,
        ALICE,
        "no list",
        &SearchConversationsRequest {
            quarantined: false,
            ..review.clone()
        },
    )
    .await;
    assert!(!in_place[0].contains(&PHISHED.to_string()), "{in_place:?}");

    // `in:` still names the place, whatever the list
    let anywhere = SearchConversationsRequest {
        query: "in:anywhere".into(),
        filter: filter(ConversationFilter {
            pinned: Some(true),
            ..Default::default()
        }),
        quarantined: false,
    };
    let got = searched(&cores, &mut d, ALICE, "pinned, in:anywhere", &anywhere).await;
    assert!(
        got[0].len() > 1,
        "in:anywhere was narrowed to the list: {got:?}"
    );

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
