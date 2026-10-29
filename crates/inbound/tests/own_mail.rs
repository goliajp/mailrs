//! A colleague's message comes back to us over port 25: the sender hands
//! every local recipient to our own MX. Until 2.135.4 any message whose
//! envelope named one of our domains and that passed SPF or *any* DKIM
//! signature skipped scoring, so a stranger signing with their own domain
//! under `MAIL FROM:<x@golia.jp>` was treated as one of us.
//!
//! Now the skip needs all three of: the peer is our own sender, DMARC
//! passed, and the passing signature is ours and aligned with a `From:`
//! we host.

use std::net::{IpAddr, Ipv4Addr};

use mailrs_inbound::{DeliveryDecision, DmarcPolicy, ReceiveContext, make_delivery_decision};

const STRANGER: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));

fn arriving(from: &str) -> ReceiveContext {
    let message = format!("From: {from}\r\nSubject: hello\r\n\r\nbody\r\n").into_bytes();
    let mut ctx = ReceiveContext::new(
        STRANGER,
        "mail.example",
        "colleague@golia.jp",
        "lihao@golia.jp",
        message,
        "mail.golia.ai",
    );
    ctx.local_domains = ["golia.jp".to_string()].into();
    ctx.content_score = 99.0;
    ctx
}

fn decided(ctx: &ReceiveContext) -> DeliveryDecision {
    make_delivery_decision(&ctx.to_pipeline_input(5.0))
}

fn proven(mut ctx: ReceiveContext) -> ReceiveContext {
    ctx.trusted_relay = true;
    ctx.own_signature = true;
    ctx.auth_results.spf = "pass".into();
    ctx.auth_results.dkim = "pass".into();
    ctx.auth_results.dmarc = "pass".into();
    ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
    ctx
}

#[test]
fn our_own_signed_mail_from_our_sender_skips_scoring() {
    let ctx = proven(arriving("colleague@golia.jp"));
    assert!(matches!(decided(&ctx), DeliveryDecision::Accept { .. }));
}

#[test]
fn each_of_the_three_alone_is_not_enough() {
    let mut no_peer = proven(arriving("colleague@golia.jp"));
    no_peer.trusted_relay = false;
    let mut no_signature = proven(arriving("colleague@golia.jp"));
    no_signature.own_signature = false;
    let mut no_dmarc = proven(arriving("colleague@golia.jp"));
    no_dmarc.auth_results.dmarc = "none".into();
    no_dmarc.auth_results.dmarc_policy = DmarcPolicy::None;
    for (what, ctx) in [
        ("a replay of our signed mail from outside", no_peer),
        (
            "our sender's address with a foreign signature",
            no_signature,
        ),
        ("our sender and signature without DMARC", no_dmarc),
    ] {
        assert!(
            matches!(decided(&ctx), DeliveryDecision::Junk { .. }),
            "{what} skipped scoring"
        );
    }
}

/// The verifier's case: our envelope domain, a stranger's own DKIM.
#[test]
fn our_envelope_domain_with_a_foreign_signature_is_scored() {
    let mut ctx = arriving("someone@stranger.example");
    ctx.auth_results.spf = "fail".into();
    ctx.auth_results.dkim = "pass".into();
    assert!(matches!(decided(&ctx), DeliveryDecision::Junk { .. }));
}

#[test]
fn a_from_of_ours_that_fails_dmarc_is_junk_even_when_allowed_by_the_recipient() {
    let mut ctx = arriving("ceo@golia.jp");
    ctx.content_score = 0.0;
    ctx.auth_results.dmarc = "fail".into();
    ctx.auth_results.dmarc_policy = DmarcPolicy::None;
    ctx.recipient_whitelist = ["ceo@golia.jp".to_string()].into();
    match decided(&ctx) {
        DeliveryDecision::Junk { reason, .. } => assert!(reason.contains("claims our domain")),
        other => panic!("a forged colleague got {other:?}"),
    }
}

/// A DNS failure is not evidence: a colleague's mail is not sent to Junk
/// because the DMARC lookup errored.
#[test]
fn a_dmarc_lookup_error_is_not_a_forgery() {
    let mut ctx = arriving("colleague@golia.jp");
    ctx.content_score = 0.0;
    ctx.auth_results.dmarc = "temperror".into();
    assert!(matches!(decided(&ctx), DeliveryDecision::Accept { .. }));
}
