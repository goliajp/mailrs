//! The admin's local sender lists, applied whether or not greylisting is on.
//!
//! Runs first in the pipeline and reads the lists once:
//!
//! - black hit (address, domain or subdomain, network; envelope sender or
//!   `From:` address) → 550 5.7.1, always.
//! - white hit by network → the sender is allowed outright: the client IP
//!   cannot be forged the way an address can.
//! - white hit by address or domain → recorded, and allowed only once the
//!   mail_auth stage proves it ([`address_authenticated`]). Without that
//!   proof a forged `From:` would carry anyone past every check.
//!
//! An allowed sender is accepted to the inbox (`make_delivery_decision`)
//! whatever the scores, the DMARC policy and the recipient's blacklist say.
//! It is not spared what is judged on the message itself: a virus is still
//! rejected, and fraud findings still hold the conversation, since a
//! sender whose account was taken over authenticates as well as ever. A
//! full mailbox is still deferred by the quota stage, which judges the
//! recipient and not the sender.
//!
//! Networks are read once more at connect (`smtp_session::admission`),
//! where a black network is refused before the greeting and a white one
//! skips the rate limit and DNSBL. Addresses and domains cannot be read
//! there: no sender is known and nothing is authenticated until DATA.

use async_trait::async_trait;
use mailrs_inbound::{AllowListMatch, DeliveryDecision, ReceiveContext, Stage, StageOutcome};

use crate::greylist_local::GreylistLocalHandle;

pub struct LocalListsStage {
    lists: GreylistLocalHandle,
}

impl LocalListsStage {
    pub fn new(lists: GreylistLocalHandle) -> Self {
        Self { lists }
    }
}

#[async_trait]
impl Stage for LocalListsStage {
    fn name(&self) -> &str {
        "local_lists"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let lists = self.lists.read().await;
        let addresses = [ctx.sender.to_lowercase(), ctx.header_from_addr.clone()];
        // black first, so one black entry punches a hole in a broad white one
        let black = addresses
            .iter()
            .find_map(|a| lists.matches_black(a, ctx.client_ip));
        if let Some(kind) = black {
            metrics::counter!("mailrs_greylist_local_black_hit_total", "kind" => kind).increment(1);
            tracing::info!(
                target: "local_lists",
                sender = %ctx.sender,
                client_ip = %ctx.client_ip,
                kind = %kind,
                "local black hit, rejecting 550"
            );
            return StageOutcome::Decide(DeliveryDecision::Reject {
                code: 550,
                message: "5.7.1 Message rejected: local policy denied".to_string(),
            });
        }
        if lists.white_network(ctx.client_ip) {
            metrics::counter!("mailrs_greylist_local_white_hit_total", "kind" => "cidr")
                .increment(1);
            ctx.allow_list = AllowListMatch::Network;
            ctx.sender_allowed = true;
        } else if let Some(a) = addresses
            .into_iter()
            .find(|a| !a.is_empty() && lists.white_address(a))
        {
            metrics::counter!("mailrs_greylist_local_white_hit_total", "kind" => "address")
                .increment(1);
            ctx.allow_list = AllowListMatch::Address(a);
        }
        StageOutcome::Continue
    }
}

/// Settle a white-listed address from what authentication vouched for:
/// the domains of a passing SPF check and of passing DKIM signatures.
pub fn settle_address_match(ctx: &mut ReceiveContext, vouched: &[&str]) {
    if let AllowListMatch::Address(listed) = &ctx.allow_list {
        ctx.sender_allowed = address_authenticated(listed, &ctx.header_from_addr, vouched);
    }
}

/// Whether authentication proves a white-listed address: the listed
/// address's domain and the `From:` address's domain (the one the reader
/// sees) must both be vouched for by a passing SPF check (the MAIL FROM
/// domain) or a passing DKIM signature (`d=`), under relaxed DMARC
/// alignment. An empty `From:` leaves only the listed address to prove.
pub fn address_authenticated(listed: &str, header_from: &str, vouched: &[&str]) -> bool {
    let domain_of = |a: &str| a.rsplit_once('@').map(|(_, d)| d.to_string());
    let Some(listed) = domain_of(listed) else {
        return false;
    };
    let shown = domain_of(header_from).unwrap_or_else(|| listed.clone());
    [listed, shown].iter().all(|d| {
        vouched
            .iter()
            .any(|v| mailrs_dmarc::align_check(v, d, mailrs_dmarc::Alignment::Relaxed).is_aligned())
    })
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::str::FromStr;

    use mailrs_inbound::{DmarcPolicy, Pipeline};

    use super::*;
    use crate::greylist_local::GreylistLocalLists;

    fn handle(fill: impl FnOnce(&mut GreylistLocalLists)) -> GreylistLocalHandle {
        let mut l = GreylistLocalLists::default();
        fill(&mut l);
        std::sync::Arc::new(tokio::sync::RwLock::new(l))
    }

    fn message(ip: &str, sender: &str, from: &str) -> ReceiveContext {
        let body = format!("From: <{from}>\r\nSubject: hi\r\n\r\nhello\r\n").into_bytes();
        let ip: IpAddr = ip.parse().unwrap();
        ReceiveContext::new(
            ip,
            "mx.example",
            sender,
            "lihao@golia.jp",
            body,
            "mx.golia.jp",
        )
    }

    /// Stands in for mail_auth: what SPF/DKIM vouched for, and a DMARC
    /// verdict, then the settle step mail_auth runs.
    struct Auth {
        vouched: Vec<&'static str>,
        dmarc: DmarcPolicy,
    }

    #[async_trait]
    impl Stage for Auth {
        fn name(&self) -> &str {
            "auth"
        }
        async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
            ctx.auth_results.dmarc_policy = self.dmarc.clone();
            settle_address_match(ctx, &self.vouched);
            StageOutcome::Continue
        }
    }

    /// Scores every message far past the Junk threshold.
    struct Spammy;

    #[async_trait]
    impl Stage for Spammy {
        fn name(&self) -> &str {
            "spammy"
        }
        async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
            ctx.content_score += 50.0;
            StageOutcome::Continue
        }
    }

    async fn run(
        lists: GreylistLocalHandle,
        vouched: Vec<&'static str>,
        dmarc: DmarcPolicy,
        mut ctx: ReceiveContext,
    ) -> DeliveryDecision {
        let p = Pipeline::builder()
            .add(LocalListsStage::new(lists))
            .add(Auth { vouched, dmarc })
            .add(Spammy)
            .build();
        p.run(&mut ctx).await
    }

    fn partner_white() -> GreylistLocalHandle {
        handle(|l| {
            l.white_domains.insert("partner.com".into());
        })
    }

    #[tokio::test]
    async fn black_rejects_with_greylisting_off() {
        let lists = handle(|l| {
            l.black_domains.insert("spam.example".into());
        });
        // the receiver's own pipeline, greylisting off (no greylist db)
        let p = crate::inbound::pipeline::build_inbound_pipeline(
            None,
            Default::default(),
            crate::greylist_sync::empty(),
            lists,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            5.0,
        );
        assert!(!p.stage_names().any(|n| n == "greylist"));
        let mut ctx = message("198.51.100.7", "x@mail.spam.example", "x@mail.spam.example");
        match p.run(&mut ctx).await {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.starts_with("5.7.1"), "{message}");
            }
            other => panic!("expected 550, got {other:?}"),
        }
        // the From header alone is enough for a black hit
        let mut ctx = message("198.51.100.7", "bounce@esp.example", "x@spam.example");
        assert!(matches!(
            p.run(&mut ctx).await,
            DeliveryDecision::Reject { code: 550, .. }
        ));
    }

    #[tokio::test]
    async fn white_domain_with_aligned_authentication_skips_junk() {
        let ctx = message(
            "198.51.100.7",
            "bounce@mail.partner.com",
            "news@partner.com",
        );
        let d = run(partner_white(), vec!["partner.com"], DmarcPolicy::Pass, ctx).await;
        assert!(matches!(d, DeliveryDecision::Accept { .. }), "{d:?}");
    }

    #[tokio::test]
    async fn white_domain_without_authentication_is_not_rescued() {
        // forged: the From claims the listed domain, nothing proves it
        let ctx = message("198.51.100.7", "ceo@partner.com", "ceo@partner.com");
        let d = run(partner_white(), vec![], DmarcPolicy::None, ctx).await;
        assert!(matches!(d, DeliveryDecision::Junk { .. }), "{d:?}");
        // proven for the forger's own domain only
        let ctx = message("198.51.100.7", "x@attacker.example", "ceo@partner.com");
        let d = run(
            partner_white(),
            vec!["attacker.example"],
            DmarcPolicy::None,
            ctx,
        )
        .await;
        assert!(matches!(d, DeliveryDecision::Junk { .. }), "{d:?}");
    }

    #[tokio::test]
    async fn white_network_is_rescued_without_authentication() {
        let lists = handle(|l| {
            l.white_cidrs
                .push(ipnet::IpNet::from_str("203.0.113.0/24").unwrap());
        });
        let ctx = message("203.0.113.9", "x@anything.example", "x@anything.example");
        let d = run(lists.clone(), vec![], DmarcPolicy::Reject, ctx).await;
        assert!(matches!(d, DeliveryDecision::Accept { .. }), "{d:?}");
        // and outside the network the same message is not
        let ctx = message("198.51.100.7", "x@anything.example", "x@anything.example");
        let d = run(lists, vec![], DmarcPolicy::Reject, ctx).await;
        assert!(matches!(d, DeliveryDecision::Reject { .. }), "{d:?}");
    }

    #[tokio::test]
    async fn black_beats_white() {
        let lists = handle(|l| {
            l.white_domains.insert("partner.com".into());
            l.black_emails.insert("ceo@partner.com".into());
        });
        let ctx = message("198.51.100.7", "ceo@partner.com", "ceo@partner.com");
        let d = run(lists, vec!["partner.com"], DmarcPolicy::Pass, ctx).await;
        assert!(matches!(d, DeliveryDecision::Reject { code: 550, .. }));
    }

    #[test]
    fn a_listed_address_needs_its_domain_and_the_from_domain_vouched_for() {
        // DKIM d=partner.com, or SPF for bounce.partner.com
        assert!(address_authenticated(
            "news@partner.com",
            "news@partner.com",
            &["partner.com"]
        ));
        assert!(address_authenticated(
            "bounce@mail.partner.com",
            "news@partner.com",
            &["mail.partner.com"]
        ));
        // forged: nothing passed, or only the forger's own domain did
        assert!(!address_authenticated(
            "ceo@partner.com",
            "ceo@partner.com",
            &[]
        ));
        assert!(!address_authenticated(
            "ceo@partner.com",
            "ceo@partner.com",
            &["attacker.example"]
        ));
        // listed envelope proven, but the From the reader sees is not
        assert!(!address_authenticated(
            "bounce@partner.com",
            "it@bank.example",
            &["partner.com"]
        ));
        assert!(address_authenticated("a@partner.com", "", &["partner.com"]));
    }
}
