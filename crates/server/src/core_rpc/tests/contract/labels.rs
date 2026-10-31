//! Labels answer alike on both cores: made and renamed with the same ids,
//! refused alike, put on conversations and listed by label, carried on the
//! rows, and gone from a conversation deleted for good.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::label::{CreateLabelRequest, UpdateLabelRequest};
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

fn mail(id: &str) -> Vec<u8> {
    format!(
        "From: Carol <carol@outside.example>\r\nTo: alice@example.test\r\n\
         Subject: {id}\r\nMessage-ID: <{id}@outside.example>\r\n\
         Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\r\nhello\r\n"
    )
    .into_bytes()
}

/// The ids in `label`'s list, sorted, and every row's labels.
async fn listed(
    c: &Cores,
    d: &mut Diffs,
    label: Option<&str>,
    step: &str,
) -> Vec<(String, Vec<String>)> {
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            label: label.map(String::from),
            ..Default::default()
        },
    };
    let got = d.both(
        &format!("list_conversations label={label:?} after {step}"),
        c.fastcore.list_conversations(ALICE, &req).await,
        c.pg_core.list_conversations(ALICE, &req).await,
        &["id", "mailbox_id", "created_at", "modseq"],
    );
    let mut rows: Vec<(String, Vec<String>)> = got
        .map(|(fc, _)| {
            fc.items
                .into_iter()
                .map(|r| (r.thread_id, r.labels))
                .collect()
        })
        .unwrap_or_default();
    rows.sort();
    rows
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn labels_answer_alike_on_both_cores() {
    let cores = harness::start_on(
        "mem://labels-fastcore-side-state",
        "mem://labels-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    for id in ["l1", "l2"] {
        spools.arrive(&cores, &envelope(None), &mail(id)).await;
    }
    let tid = |id: &str| format!("{id}@outside.example");
    let make = |name: &str, color: &str| CreateLabelRequest {
        name: name.into(),
        color: color.into(),
    };

    for (name, color) in [("Work", "blue"), ("Home", "green")] {
        d.both(
            &format!("create_label {name}"),
            cores.fastcore.create_label(ALICE, &make(name, color)).await,
            cores.pg_core.create_label(ALICE, &make(name, color)).await,
            &["created_at"],
        );
    }
    for bad in [make("work", "red"), make("Inbox", "red"), make("x", "pink")] {
        d.both(
            &format!("create_label {} refused", bad.name),
            cores.fastcore.create_label(ALICE, &bad).await,
            cores.pg_core.create_label(ALICE, &bad).await,
            &[],
        );
    }
    let rename = UpdateLabelRequest {
        name: Some("Jobs".into()),
        color: None,
    };
    d.both(
        "update_label",
        cores.fastcore.update_label(ALICE, "1", &rename).await,
        cores.pg_core.update_label(ALICE, "1", &rename).await,
        &["created_at"],
    );
    d.both(
        "list_labels",
        cores.fastcore.list_labels(ALICE).await,
        cores.pg_core.list_labels(ALICE).await,
        &["created_at"],
    );

    for c in [&cores.fastcore, &cores.pg_core] {
        c.set_thread_label(ALICE, &tid("l1"), "1", true)
            .await
            .expect("label");
        c.set_thread_label(ALICE, &tid("l1"), "2", true)
            .await
            .expect("label");
        c.set_thread_label(ALICE, &tid("l2"), "2", true)
            .await
            .expect("label");
        c.archive_thread(ALICE, &tid("l2")).await.expect("archive");
    }
    d.both(
        "label_thread on a missing label",
        cores
            .fastcore
            .set_thread_label(ALICE, &tid("l1"), "99", true)
            .await,
        cores
            .pg_core
            .set_thread_label(ALICE, &tid("l1"), "99", true)
            .await,
        &[],
    );
    let home = listed(&cores, &mut d, Some("2"), "labelling").await;
    assert_eq!(
        home.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
        [tid("l1"), tid("l2")],
        "a label lists archived conversations too"
    );
    let all = listed(&cores, &mut d, None, "labelling").await;
    assert_eq!(all, [(tid("l1"), vec!["2".to_string(), "1".to_string()])]);

    for c in [&cores.fastcore, &cores.pg_core] {
        c.set_thread_label(ALICE, &tid("l1"), "2", false)
            .await
            .expect("unlabel");
        c.delete_thread(ALICE, &tid("l2")).await.expect("delete");
    }
    assert!(
        listed(&cores, &mut d, Some("2"), "unlabel and delete")
            .await
            .is_empty()
    );
    d.both(
        "delete_label",
        cores.fastcore.delete_label(ALICE, "1").await,
        cores.pg_core.delete_label(ALICE, "1").await,
        &[],
    );
    let all = listed(&cores, &mut d, None, "delete_label").await;
    assert_eq!(all, [(tid("l1"), Vec::<String>::new())]);
    assert!(d.0.is_empty(), "the cores differ:\n{}", d.0.join("\n"));
}
