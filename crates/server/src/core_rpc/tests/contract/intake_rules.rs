//! A user's rules run alike on both cores as mail arrives through the spool,
//! and held mail is left alone.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;
use mailrs_core_sidestate::families::rules::{Draft, create};

use super::harness::{self, Diffs};
use super::intake::{PHISH, REPORT, Spools, envelope, held};
use super::{ALICE, accounts};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rules_run_alike_on_both_cores_as_mail_arrives() {
    let (fc_kevy, pg_kevy) = (
        "mem://rules-fastcore-side-state",
        "mem://rules-pg-core-side-state",
    );
    let cores = harness::start_on(fc_kevy, pg_kevy).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    for url in [fc_kevy, pg_kevy] {
        let mut conn = kevy_client::Connection::connect(url).expect("kevy");
        for (name, query, actions) in [
            (
                "report",
                "from:carol subject:quarterly",
                vec!["star", "read"],
            ),
            ("bank", "from:alerts", vec!["archive"]),
        ] {
            let draft = Draft {
                name,
                query,
                actions: actions.into_iter().map(String::from).collect(),
                enabled: true,
            };
            create(&mut conn, ALICE, draft, 1)
                .expect("store")
                .expect("rule");
        }
    }

    spools.arrive(&cores, &envelope(None), REPORT).await;
    spools.arrive(&cores, &envelope(held()), PHISH).await;

    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    let (_, rows) = d
        .both(
            "list_conversations after rules ran",
            cores.fastcore.list_conversations(ALICE, &req).await,
            cores.pg_core.list_conversations(ALICE, &req).await,
            MINTED,
        )
        .expect("both list");
    let report = rows
        .items
        .iter()
        .find(|r| r.thread_id == "q1@outside.example")
        .expect("the report is listed");
    assert!(report.flagged, "the rule did not star it");
    assert_eq!(report.unread_count, 0, "the rule did not mark it read");
    let held_rows = d.both(
        "list_quarantined after rules ran",
        cores.fastcore.list_quarantined(ALICE, 50, None).await,
        cores.pg_core.list_quarantined(ALICE, 50, None).await,
        MINTED,
    );
    let json = serde_json::to_string(&held_rows.expect("held").1).expect("json");
    assert!(json.contains("p1@outside.example"), "{json}");
    assert!(
        !json.contains("\"archived\":true"),
        "a rule archived held mail: {json}"
    );

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
