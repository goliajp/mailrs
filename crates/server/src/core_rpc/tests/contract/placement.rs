//! Both cores store the same record of why a delivered message was filed
//! where it was: the receiver's Junk reason, the reader's sender rule, or
//! the reader's filter.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_sidestate::families::sender_rules::{SenderRule, put};

use super::harness::{self, Diffs};
use super::intake::{PHISH, REPORT, Spools, envelope};
use super::{ALICE, accounts};

const NOTE: &[u8] = b"From: Dan <dan@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: lunch\r\n\
Message-ID: <n1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 13:00:00 +0900\r\n\
\r\n\
Noon?\r\n";

fn junked_by_the_receiver() -> Option<String> {
    Some(
        serde_json::json!({
            "rules_version": "2026-08-29.1",
            "score": 0.0,
            "quarantined": false,
            "layers": [{
                "name": "identity", "outcome": "fail", "score": 4.5,
                "detail": "display name claims a company, from a domain that is not theirs and is new here",
                "reasons": [{"code": "impersonates-brand", "params": {"brand": "paypal", "domain": "outside.example"}}],
            }],
            "placement": [{"code": "junk-score", "params": {"score": "6.0", "threshold": "5.0"}}],
        })
        .to_string(),
    )
}

fn into(folder: &str, verdict: Option<String>) -> SpoolEnvelope {
    SpoolEnvelope {
        target_folder: folder.into(),
        ..envelope(verdict)
    }
}

fn placement_codes(v: &serde_json::Value) -> Vec<String> {
    v["verdict"]["placement"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r["code"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_cores_store_why_each_message_was_filed() {
    let (fc_kevy, pg_kevy) = (
        "mem://placement-fastcore-side-state",
        "mem://placement-pg-core-side-state",
    );
    let cores = harness::start_on(fc_kevy, pg_kevy).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    for url in [fc_kevy, pg_kevy] {
        let mut conn = kevy_client::Connection::connect(url).expect("kevy");
        let rule = SenderRule {
            bucket: Some("promotions".into()),
            ..Default::default()
        };
        put(&mut conn, ALICE, "carol@outside.example", &rule).expect("rule");
    }

    spools.arrive(&cores, &into("INBOX", None), REPORT).await;
    spools
        .arrive(&cores, &into("Junk", junked_by_the_receiver()), PHISH)
        .await;
    spools.arrive(&cores, &into("Junk", None), NOTE).await;

    for (mid, want) in [
        ("q1@outside.example", "sorted-by-your-rule"),
        ("p1@outside.example", "junk-score"),
        ("n1@outside.example", "junk-your-filter"),
    ] {
        let (fc, pg) = d
            .both(
                &format!("fraud_verdict {mid}"),
                cores.fastcore.fraud_verdict(ALICE, mid).await,
                cores.pg_core.fraud_verdict(ALICE, mid).await,
                &[],
            )
            .expect("both read");
        assert_eq!(placement_codes(&fc), [want], "fastcore {mid}: {fc}");
        assert_eq!(placement_codes(&pg), [want], "pg-core {mid}: {pg}");
        if mid == "p1@outside.example" {
            // a rule's reason keeps the facts it named, in both cores
            for v in [&fc, &pg] {
                assert_eq!(
                    v["verdict"]["layers"][0]["reasons"][0]["params"],
                    serde_json::json!({"brand": "paypal", "domain": "outside.example"}),
                    "{v}"
                );
            }
        }
    }
    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
