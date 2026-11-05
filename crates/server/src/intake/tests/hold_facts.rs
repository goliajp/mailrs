//! Each hold appends one fact, with what the reader had unread then.

use super::Fixture;

const ALICE: &str = "alice@example.com";

const REPORT: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.com\r\n\
Subject: Quarterly report\r\n\
Message-ID: <q1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Please review the attached figures before Friday.\r\n";

const AGAIN: &[u8] = b"From: Carol <carol@outside.example>\r\n\
Subject: Re: Quarterly report\r\n\
Message-ID: <q2@outside.example>\r\n\
In-Reply-To: <q1@outside.example>\r\n\
References: <q1@outside.example>\r\n\
\r\n\
again\r\n";

/// A second held message in the same conversation holds nothing new and
/// adds no fact.
#[tokio::test(flavor = "multi_thread")]
async fn a_hold_is_one_fact_with_the_unread_message_ids() {
    let f = Fixture::new().await;
    f.account(ALICE).await;
    let verdict = serde_json::json!({
        "rules_version": "2026-11-05.1",
        "score": 9.5,
        "quarantined": true,
        "layers": [],
    })
    .to_string();
    let mut env = Fixture::envelope(&[ALICE]);
    env.fraud_verdict = Some(verdict);
    f.spool_env("new", "1790000002.M1P1Q1.host", &env, REPORT);
    assert_eq!(f.tick().await, 1);
    f.spool_env("new", "1790000003.M1P1Q1.host", &env, AGAIN);
    assert_eq!(f.tick().await, 1);

    let facts = f
        .core
        .mailbox
        .holds(ALICE, "q1@outside.example")
        .await
        .expect("facts");
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].source, "ingest");
    assert_eq!(facts[0].rules_version, "2026-11-05.1");
    assert_eq!(facts[0].unread_mids, ["q1@outside.example"]);
    assert!(facts[0].at > 0);
}
