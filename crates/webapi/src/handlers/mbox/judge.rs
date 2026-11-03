//! Imported mail through the receive path's checks.
//!
//! An import is mail from outside that arrives as stored mail, so it is
//! judged the way the core's re-scan judges stored mail
//! (`mailrs_fraud_lua::stored`) and screened the way the replay screens it
//! (`mailrs_screening_lua::stored`), under the rules and policy the
//! receiver runs. The verdict goes to the core with the message, as the
//! receiver's goes with a delivery, and the core holds what it says to.
//!
//! **Nothing here records.** Sender history is read and never counted,
//! the screening sets are read and no observation is added: imported
//! spam must not teach this deployment anything about who sends spam.

/// One import's checks, set up once.
pub struct Judge {
    policy: mailrs_fraud::Policy,
    our_stamp: Option<String>,
    hist: Option<kevy_client::Connection>,
    screening: mailrs_core_sidestate::families::screening::Live,
    own: Vec<String>,
}

/// What the checks made of one message.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// The verdict to hand the core, when the fraud rules found anything.
    pub fraud_verdict: Option<String>,
    /// The verdict holds the conversation.
    pub holds: bool,
}

/// A comma-separated environment variable, or nothing.
fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

impl Judge {
    /// `account_names` are the display names on this deployment's accounts,
    /// the one part of the policy the store holds rather than the
    /// environment.
    pub fn new(account_names: Vec<String>) -> Self {
        let own: Vec<String> = csv_env("MAILRS_LOCAL_DOMAINS")
            .into_iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        let policy = mailrs_fraud::Policy {
            org_names: csv_env("MAILRS_ORG_NAMES"),
            our_domains: own.clone(),
            allowed_domains: csv_env("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
            account_names,
        };
        if policy.org_names.is_empty() {
            tracing::warn!(
                "mbox import: MAILRS_ORG_NAMES is empty — imported mail is judged without the \
                 impersonation check"
            );
        }
        let our_stamp = std::env::var("MAILRS_HOSTNAME")
            .ok()
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty());
        let mut hist = std::env::var("MAILRS_KEVY_URL")
            .ok()
            .and_then(|u| kevy_client::Connection::connect(&u).ok());
        let screening = mailrs_core_sidestate::families::screening::Live::new(own.clone());
        if let Some(conn) = hist.as_mut() {
            screening.refresh(conn, chrono::Utc::now().timestamp());
        }
        Self {
            policy,
            our_stamp,
            hist,
            screening,
            own,
        }
    }

    /// Judge and screen one received message. A rule set that cannot run
    /// lets the message through, as it does on the receive path.
    pub fn check(&mut self, user: &str, raw: &[u8]) -> Checked {
        self.screen(user, raw);
        // judged before the core files it: this message is not counted yet
        let judged = match mailrs_fraud_lua::stored::judge_for(
            raw,
            Some((user, mailrs_fraud_lua::stored::relation::Counted::Before)),
            self.our_stamp.as_deref(),
            &[],
            &self.policy,
            &mut self.hist,
        ) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(%user, error = %e, "mbox import: the fraud rules could not run");
                return Checked::default();
            }
        };
        if judged.findings.is_empty() {
            return Checked::default();
        }
        Checked {
            fraud_verdict: serde_json::to_string(&judged.verdict).ok(),
            holds: judged.holds,
        }
    }

    /// The screening rules, in the mode they run in on arrival: shadow,
    /// which logs what they would have done.
    fn screen(&self, user: &str, raw: &[u8]) {
        let envelope_domain = mailrs_fraud_lua::stored::header_value(raw, b"return-path")
            .and_then(|v| {
                v.trim()
                    .trim_matches(['<', '>'])
                    .rsplit_once('@')
                    .map(|(_, d)| d.to_ascii_lowercase())
            })
            .unwrap_or_default();
        let verdict = mailrs_screening_lua::stored::with_input(
            &self.screening.reputation(),
            &self.screening.inspector(),
            &self.own,
            raw,
            &envelope_domain,
            self.our_stamp.as_deref(),
            mailrs_screening_lua::evaluate,
        );
        let verdict = match verdict {
            Ok(v) => v,
            Err(e) => {
                self.screening.failed("rules", &e);
                return;
            }
        };
        let message_id =
            mailrs_fraud_lua::stored::header_value(raw, b"message-id:").unwrap_or_default();
        for h in &verdict.hits {
            tracing::info!(
                event = "screening_shadow",
                source = "import",
                %user,
                %message_id,
                rule = %h.rule,
                kind = %h.kind,
                action = ?h.action,
                score = h.score,
                detail = %h.detail,
                evidence = %h.evidence.join(" | "),
                version = %verdict.version,
                "screening (shadow)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules the receiver runs, reading stored mail: a sender on a
    /// domain reserved for testing holds; an ordinary one carries no
    /// verdict at all, as a clean delivery does not.
    #[test]
    fn held_mail_carries_a_holding_verdict_and_clean_mail_none() {
        let mut j = Judge::new(Vec::new());
        let held = j.check(
            "a@x.test",
            b"From: Prize <win@prize.test>\r\nSubject: You won\r\n\r\nClaim now.\r\n",
        );
        assert!(held.holds);
        let v: serde_json::Value =
            serde_json::from_str(held.fraud_verdict.as_deref().expect("a verdict")).unwrap();
        assert_eq!(v["quarantined"], true, "{v}");

        let clean = j.check(
            "a@x.test",
            b"From: Erin <erin@oldmail.net>\r\nSubject: Lunch\r\n\r\nFriday?\r\n",
        );
        assert_eq!(clean, Checked::default());
    }
}
