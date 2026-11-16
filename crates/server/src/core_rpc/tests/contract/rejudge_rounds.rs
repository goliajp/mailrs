//! Re-judging after a reputation change, alike on both cores: the same
//! mail, arriving through each core's intake, is found by the same
//! observation, judged by the same rules, and given the same verdicts; the
//! batch routes answer alike.

use std::sync::Arc;

use mailrs_intake::rejudge::{judge::Judge, tick};
use mailrs_reputation::{Kind, Observation, Source, Stance};

use super::harness;
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

const FASTCORE: &str = "mem://rejudge-fastcore";
const PG_CORE: &str = "mem://rejudge-pg-core";

fn letter(n: u32, from: &str) -> Vec<u8> {
    format!(
        "Received: from mx.example (mx.example [192.0.2.1])\r\n\tby mail.contract.test; Mon, 2 Nov 2026 00:00:0{n} +0000\r\n\
         From: <{from}>\r\nTo: <{ALICE}>\r\nSubject: rejudge {n}\r\n\
         Message-ID: <rejudge-{n}@contract.test>\r\nDate: Mon, 2 Nov 2026 00:00:0{n} +0000\r\n\
         Content-Type: text/plain\r\n\r\nhello {n}\r\n"
    )
    .into_bytes()
}

fn observe(url: &str, domain: &str) {
    let mut c = kevy_client::Connection::connect(url).expect("side state");
    mailrs_core_sidestate::families::screening::add_observation(
        &mut c,
        Observation {
            id: String::new(),
            at: chrono::Utc::now().timestamp(),
            source: Source::Manual,
            kind: Kind::SenderDomain,
            value: domain.into(),
            stance: Stance::Deny,
            retracts: None,
            reason: "contract".into(),
            actor: Some("admin@example.test".into()),
        },
    )
    .expect("observation");
}

/// One re-judge round on `store`, as the re-judge thread runs it.
async fn round<S: mailrs_intake::rejudge::RejudgeStore>(store: Arc<S>) -> u64 {
    tokio::task::spawn_blocking(move || {
        let mut judge = Judge::new(&*store);
        tick(&*store, &mut judge).expect("round")
    })
    .await
    .expect("round task")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_cores_judge_the_same_mail_again_alike() {
    let c = harness::start_on(FASTCORE, PG_CORE).await;
    let mut d = harness::Diffs::default();
    accounts::seed(&c, &mut d).await;
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
    let spools = Spools::new(&c);
    for (n, from) in [
        (1, "news@mail.shop.example"),
        (2, "billing@shop.example"),
        (3, "friend@elsewhere.example"),
    ] {
        spools.arrive(&c, &envelope(None), &letter(n, from)).await;
    }
    let fc = c.fastcore_state.clone();
    let pg = Arc::new(c.pg_core_state.pg_core());
    // a store that has never run starts from the observations already there
    assert_eq!(round(fc.clone()).await, 0, "fastcore's first round");
    assert_eq!(round(pg.clone()).await, 0, "pg-core's first round");

    observe(FASTCORE, "shop.example");
    observe(PG_CORE, "shop.example");
    let judged = (round(fc.clone()).await, round(pg.clone()).await);
    assert_eq!(judged, (2, 2), "messages judged (fastcore, pg-core)");

    let m = |n: u32| format!("rejudge-{n}@contract.test");
    for n in 1..=2 {
        let kevy = fc
            .mailbox
            .ledger_verdict(ALICE, &m(n))
            .expect("kevy verdict");
        let sql = c
            .pg_core_state
            .mailbox
            .ledger_verdict(ALICE, &m(n))
            .await
            .expect("sql verdict");
        let (kevy, sql) = (
            kevy.expect("fastcore verdict"),
            sql.expect("pg-core verdict"),
        );
        assert_eq!(
            (
                &kevy.rules,
                kevy.holds,
                &kevy.rules_version,
                &kevy.screening
            ),
            (&sql.rules, sql.holds, &sql.rules_version, &sql.screening),
            "re-judged verdict of letter {n}"
        );
    }
    // nothing more to judge until another observation
    assert_eq!((round(fc).await, round(pg).await), (0, 0), "a quiet round");

    let batches = d.both(
        "rejudge_batches",
        c.fastcore.rejudge_batches().await,
        c.pg_core.rejudge_batches().await,
        &[],
    );
    assert!(batches.is_some(), "the batch list answers on both cores");
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
}
