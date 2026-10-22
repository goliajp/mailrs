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

mod accounts;
mod coverage;
mod harness;
mod intake;
mod mail;
mod sent;
mod side;
mod threads;

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
    mail::run(&cores, &mut d).await;
    threads::run(&cores, &mut d).await;
    side::run(&cores, &mut d).await;
    sent::run(&cores, &mut d).await;
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
