//! DMARC: policy TXT lookup + policy enforcement + aggregate recording.

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mailrs_dmarc::{DmarcOutcome, DmarcPolicy as MailrsPolicy, PolicyAction};
use mailrs_inbound::{DeliveryDecision, DmarcPolicy, ReceiveContext, StageOutcome};

use super::MailAuthStage;

/// The DMARC record that governs mail from `from_domain`, and the domain
/// it was published at. A domain without a record of its own falls under
/// its organizational domain's (RFC 7489 §6.6.3), whose `sp=` then
/// applies. `None` when neither publishes one, which behaves as `p=none`.
pub(super) async fn lookup_policy(
    resolver: &TokioResolver,
    from_domain: &str,
) -> Option<(MailrsPolicy, String)> {
    let (record, at) = governing_record(from_domain, |name| txt_records(resolver, name)).await?;
    MailrsPolicy::parse(&record).ok().map(|p| (p, at))
}

async fn txt_records(resolver: &TokioResolver, name: String) -> Vec<String> {
    let Ok(lookup) = resolver.txt_lookup(name).await else {
        return Vec::new();
    };
    lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::TXT(t) => Some(t.to_string()),
            _ => None,
        })
        .collect()
}

async fn governing_record<F, Fut>(from_domain: &str, txt: F) -> Option<(String, String)>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Vec<String>>,
{
    if let Some(r) = dmarc_record(txt(format!("_dmarc.{from_domain}")).await) {
        return Some((r, from_domain.to_string()));
    }
    let org = mailrs_dmarc::organizational_domain(from_domain)?;
    if org.eq_ignore_ascii_case(from_domain) {
        return None;
    }
    let r = dmarc_record(txt(format!("_dmarc.{org}")).await)?;
    Some((r, org))
}

/// The one record that starts `v=DMARC1`; other TXT records at the name
/// are ignored, and more than one DMARC record means none (§6.6.3).
fn dmarc_record(records: Vec<String>) -> Option<String> {
    let mut dmarc = records.into_iter().filter(|r| {
        r.trim_start()
            .get(..8)
            .is_some_and(|v| v.eq_ignore_ascii_case("v=DMARC1"))
    });
    let first = dmarc.next()?;
    match dmarc.next() {
        Some(_) => None,
        None => Some(first),
    }
}

impl MailAuthStage {
    /// Apply the DMARC policy from `outcome` to the receive context.
    /// `pass` → label and continue; `reject` → short-circuit with 5.7.1
    /// reject (and record the result); `quarantine` / `none` → label
    /// and continue. All non-pass outcomes also record the result for
    /// aggregate reporting.
    pub(super) async fn apply_dmarc_policy(
        &self,
        ctx: &mut ReceiveContext,
        outcome: &DmarcOutcome,
        mail_from_domain: &str,
    ) -> StageOutcome {
        if outcome.dmarc_pass {
            ctx.auth_results.dmarc = "pass".into();
            ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
        } else {
            match outcome.disposition {
                PolicyAction::Reject => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Reject;

                    tracing::info!(
                        event = "dmarc_reject",
                        domain = mail_from_domain,
                        spf = %ctx.auth_results.spf,
                        dkim = %ctx.auth_results.dkim,
                        reason = %outcome.reason,
                        "DMARC reject"
                    );

                    // an allowed sender is accepted all the same, by the final decision
                    if ctx.sender_allowed {
                        return StageOutcome::Continue;
                    }
                    return StageOutcome::Decide(DeliveryDecision::Reject {
                        code: 550,
                        message: format!("5.7.1 DMARC policy reject for domain {mail_from_domain}"),
                    });
                }
                PolicyAction::Quarantine => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Quarantine;
                }
                PolicyAction::None => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::None;
                }
            }
        }

        StageOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    async fn governing(from: &str, dns: &[(&str, &str)]) -> Option<(String, String)> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (name, txt) in dns {
            map.entry(name.to_string())
                .or_default()
                .push(txt.to_string());
        }
        governing_record(from, |name| {
            let found = map.get(&name).cloned().unwrap_or_default();
            async move { found }
        })
        .await
    }

    #[tokio::test]
    async fn a_subdomain_without_a_record_falls_under_its_organization() {
        let dns = [("_dmarc.example.com", "v=DMARC1; p=reject; sp=quarantine")];
        let (record, at) = governing("mail.example.com", &dns).await.unwrap();
        assert_eq!(at, "example.com");
        assert!(record.contains("sp=quarantine"));
        // a record of its own wins
        let own = [
            ("_dmarc.example.com", "v=DMARC1; p=reject"),
            ("_dmarc.mail.example.com", "v=DMARC1; p=none"),
        ];
        assert_eq!(
            governing("mail.example.com", &own).await.unwrap().1,
            "mail.example.com"
        );
        // the organization is found through the public suffix list
        let jp = [("_dmarc.example.co.jp", "v=DMARC1; p=reject")];
        assert_eq!(
            governing("a.b.example.co.jp", &jp).await.unwrap().1,
            "example.co.jp"
        );
        assert_eq!(governing("example.co.jp", &[]).await, None);
    }

    #[tokio::test]
    async fn only_one_record_that_starts_v_dmarc1_counts() {
        let other = [
            ("_dmarc.example.com", "google-site-verification=abc"),
            ("_dmarc.example.com", "v=DMARC1; p=quarantine"),
        ];
        assert!(
            governing("example.com", &other)
                .await
                .unwrap()
                .0
                .contains("quarantine")
        );
        let two = [
            ("_dmarc.example.com", "v=DMARC1; p=reject"),
            ("_dmarc.example.com", "v=DMARC1; p=none"),
        ];
        assert_eq!(governing("example.com", &two).await, None);
    }
}
