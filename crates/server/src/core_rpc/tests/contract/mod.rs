//! The two cores are interchangeable behind the core-api.
//!
//! Every method of the core-api client is called on fastcore and on the
//! SQL core with the same arguments, after the same seed, and the answers
//! are compared field by field; the conversation views are compared again
//! after every action that changes them. A difference that is legitimate
//! (an id each store mints) is named where the call is made.
//!
//! One test, run in order, because the scenarios build on one mailbox and
//! the coverage check at the end needs to have seen all of them: every path
//! the client can ask for must have been asked of both cores.

mod account_audit;
mod account_purge;
mod accounts;
mod attachment_text;
mod callback_numbers;
mod coverage;
mod deleted_rows;
mod deletes;
mod display;
mod flag_lists;
mod forged_results;
mod group_purge;
mod harness;
mod held_read_state;
mod import;
mod importance;
mod intake;
mod intake_redrain;
mod intake_rules;
mod junk_corrections;
mod labels;
mod mail;
mod mark_list_read;
mod operator;
mod ops;
mod own_unread;
mod ownerless;
mod paging;
mod participants;
mod placement;
mod purge_fixture;
mod quota;
mod quota_limits;
mod rejudge;
mod reports;
mod search;
mod search_scope;
mod sections;
mod sender_rules;
mod sent;
mod sent_to;
mod shared;
mod shared_read_state;
mod side;
mod side_drafts;
mod standing;
mod submission;
mod threads;
mod trash;
mod usage_drift;
mod webhooks;

const DOMAIN: &str = "example.test";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_two_cores_answer_every_client_call_alike() {
    let cores = harness::start().await;
    let mut d = harness::Diffs::default();

    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;
    accounts::run(&cores, &mut d).await;
    quota_limits::run(&cores, &mut d).await;
    mail::run(&cores, &mut d).await;
    threads::run(&cores, &mut d).await;
    side::run(&cores, &mut d).await;
    sent::run(&cores, &mut d).await;
    import::run(&cores, &mut d).await;
    forged_results::run(&cores, &mut d).await;
    sender_rules::run(&cores, &mut d).await;
    sections::run(&cores, &mut d).await;
    shared::run(&cores, &mut d).await;
    webhooks::run(&cores, &mut d).await;
    ownerless::run(&cores, &mut d).await;
    rejudge::run(&cores, &mut d).await;
    reports::run(&cores, &mut d).await;
    usage_drift::run(&cores, &mut d).await;
    sent_to::run(&cores, &mut d).await;
    account_purge::run(&cores, &mut d).await;
    group_purge::run(&cores, &mut d).await;
    ops::run(&cores, &mut d).await;
    sections::fixtures(&cores, &mut d).await;
    accounts::teardown(&cores, &mut d).await;
    d.both(
        "healthz",
        cores.fastcore.healthz().await.map(|_| ()),
        cores.pg_core.healthz().await.map(|_| ()),
        &[],
    );
    d.both(
        "readyz",
        cores.fastcore.readyz().await.map(|_| ()),
        cores.pg_core.readyz().await.map(|_| ()),
        &[],
    );

    let asked = coverage::client_paths();
    let mut gaps = Vec::new();
    for (core, hits) in [
        ("fastcore", &cores.fastcore_hits),
        ("pg-core", &cores.pg_core_hits),
    ] {
        let hits = hits.lock().expect("hits").clone();
        for p in coverage::unexercised(&asked, &hits) {
            gaps.push(format!("{core}: no scenario reached {p}"));
        }
    }

    let mut report = d.0;
    report.extend(gaps);

    assert!(
        report.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        report.len(),
        report.join("\n\n")
    );
}
