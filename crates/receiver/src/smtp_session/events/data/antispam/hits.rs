//! What the enforced fraud rules did at delivery time, for the people
//! reviewing a rule while it is new: one `fraud_hit` log line per rule
//! that fired, and a per-rule daily counter in the shared store that
//! `/api/health` reads. Shadow hits are logged on their own
//! (`fraud_shadow`) and never counted here. Only the live path records;
//! a rejudge of old mail does not.

use std::sync::Arc;

use crate::kevy_net::KevyNetClient;

/// One rule that fired on one message, as the log line spells it.
#[derive(Debug, PartialEq)]
pub(super) struct Hit {
    pub rule: String,
    pub held: bool,
}

/// The enforced rules that fired, each with whether the message as a
/// whole is held: the same `held` on every line, because the hold is the
/// message's, not the rule's.
pub(super) fn hits(findings: &mailrs_fraud::Findings) -> Vec<Hit> {
    let held = mailrs_inbound::holds(findings);
    findings
        .iter()
        .map(|f| Hit {
            rule: f.rule.clone(),
            held,
        })
        .collect()
}

/// Log every hit and add it to today's counter. The counter is
/// bookkeeping on the receive path: a store that cannot be reached is
/// logged and the message goes on.
pub(super) async fn record(
    raw: &[u8],
    hits: Vec<Hit>,
    sender_domain: &str,
    version: &str,
    client: Option<Arc<KevyNetClient>>,
) {
    if hits.is_empty() {
        return;
    }
    let message_id = crate::screening::message_id(raw);
    for hit in &hits {
        tracing::info!(
            event = "fraud_hit",
            %message_id,
            rule = %hit.rule,
            sender_domain = %sender_domain,
            held = hit.held,
            version = %version,
            "fraud rule hit"
        );
    }
    let Some(client) = client else {
        return;
    };
    let rules: Vec<String> = hits.into_iter().map(|h| h.rule).collect();
    count(client, rules, chrono::Utc::now().timestamp()).await;
}

async fn count(client: Arc<KevyNetClient>, rules: Vec<String>, now: i64) {
    let done = tokio::task::spawn_blocking(move || {
        client.with_conn(|c| mailrs_core_sidestate::families::fraud_hits::record(c, &rules, now))
    })
    .await;
    match done {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "fraud hit counter not updated"),
        Err(error) => tracing::warn!(%error, "fraud hit counter task failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_fraud::{Finding, Findings, Layer};

    #[test]
    fn every_enforced_rule_that_fired_is_one_hit_and_the_hold_is_the_messages() {
        let mut f = Findings::new();
        f.push(Finding::new(
            "svg-runs-script",
            Layer::Content,
            6.0,
            "script",
        ));
        f.push(Finding::scored(
            "new-domain-login-link",
            Layer::Provenance,
            2.0,
            "new",
        ));
        assert_eq!(
            hits(&f),
            vec![
                Hit {
                    rule: "svg-runs-script".into(),
                    held: true
                },
                Hit {
                    rule: "new-domain-login-link".into(),
                    held: true
                },
            ]
        );
    }

    #[test]
    fn a_scored_hit_alone_is_not_held() {
        let mut f = Findings::new();
        f.push(Finding::scored(
            "bec-pay-to-a-stranger",
            Layer::Provenance,
            3.0,
            "pay",
        ));
        assert_eq!(
            hits(&f),
            vec![Hit {
                rule: "bec-pay-to-a-stranger".into(),
                held: false
            }]
        );
    }

    #[test]
    fn nothing_found_is_no_hit() {
        assert!(hits(&Findings::new()).is_empty());
    }

    #[tokio::test]
    async fn a_hit_adds_one_to_todays_counter() {
        let client = Arc::new(KevyNetClient::new("mem://receiver-fraud-hits"));
        let now = 1_793_750_400;
        count(client.clone(), vec!["svg-runs-script".into()], now).await;
        let rules = vec![
            "svg-runs-script".to_string(),
            "html-builds-a-download".into(),
        ];
        let got = tokio::task::spawn_blocking(move || {
            client
                .with_conn(|c| mailrs_core_sidestate::families::fraud_hits::counts(c, &rules, now))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(got.today["svg-runs-script"], 1);
        assert_eq!(got.today["html-builds-a-download"], 0);
    }
}
