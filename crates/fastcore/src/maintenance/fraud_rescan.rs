//! Re-evaluate historical mail with the same Lua rules as the receiver.
//!
//! Hot reload changes future scans; existing verdicts change only when
//! this bounded sweep is explicitly applied.
//!
//! # Dry by default
//!
//! The first run reports and changes nothing. That is not politeness —
//! it is the only way to see what a new signal would have done to a
//! real mailbox before it does it. Pass `dry_run=false` to move
//! them.
//!
//! # Bounded, and it pauses
//!
//! Same shape as `backfill-decode-headers` and for the same reason: an
//! unbounded sweep over this mailbox took the mail service down for
//! half an hour on 2026-08-26. `limit` threads per call, a pause every
//! `PAUSE_EVERY`, and `next_skip` in the answer.
//!
//! # What it reads
//!
//! The newest message of each thread, from that user's own maildir
//! file. The display name has to be decoded before it can be compared —
//! the names arrive base64'd — and the `X-Mailer` is read raw.

use std::collections::HashMap;

use super::prelude::*;

pub(super) mod reading;
use config::{our_authserv_id, policy_from_env};
use holding::move_to_junk;
use reading::*;

/// Threads between pauses.
const PAUSE_EVERY: u64 = 25;

pub(crate) use decision::*;

mod config;
mod decision;
mod facts;
mod holding;
mod sweep;
pub(crate) use sweep::rescan;

pub(crate) fn snapshot(state: &FastcoreState) -> std::io::Result<Vec<(String, String)>> {
    let mut targets = Vec::new();
    for user in state.mailbox.list_account_addresses()? {
        for tid in state.mailbox.all_thread_ids_for_user(&user)? {
            targets.push((user.clone(), tid));
        }
    }
    targets.sort();
    Ok(targets)
}

/// `POST /v1/admin/maintenance:fraud-rescan?dry_run=false`
pub(crate) async fn fraud_rescan_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RescanQuery>,
) -> axum::response::Response {
    rescan(state, q, None, None).await
}

#[cfg(test)]
mod tests {

    /// Everything the sweep reads off a stored message, in one place,
    /// so a test can hand it the bytes production stored.
    fn facts_as_the_sweep_reads_them(raw: &[u8], our_stamp: &str) -> (String, bool, bool, String) {
        let stamp = mailrs_inbound::identity::auth_results_authserv(raw);
        let (_, _, dmarc) = mailrs_inbound::identity::auth_results_tokens(raw);
        (
            mailrs_inbound::from_header(raw),
            stamp.as_deref() == Some(our_stamp),
            mailrs_inbound::identity::first_received_peer_ip(raw)
                .is_some_and(mailrs_inbound::identity::address_is_private),
            dmarc,
        )
    }

    fn holds_as_the_sweep_would(raw: &[u8]) -> bool {
        let (from, unauthenticated, peer_is_private, dmarc) =
            facts_as_the_sweep_reads_them(raw, "mail.golia.ai");
        let facts = mailrs_fraud::Facts {
            from: &from,
            subject: &mailrs_inbound::subject_header(raw),
            unauthenticated,
            peer_is_private,
            dmarc: &dmarc,
            ..mailrs_fraud::Facts::default()
        };
        let policy = mailrs_fraud::Policy {
            our_domains: vec!["golia.jp".into(), "golia.ai".into()],
            ..mailrs_fraud::Policy::default()
        };
        let mut rules = mailrs_fraud_lua::Rules::compile(mailrs_fraud_lua::DEFAULT_SOURCE)
            .expect("shipped bundle compiles");
        let findings = rules.classify(&facts, &policy).findings;
        assert!(
            findings.has(mailrs_fraud::RULE_CLAIMS_OUR_DOMAIN) == findings.hold_worthy()
                || !findings.has(mailrs_fraud::RULE_CLAIMS_OUR_DOMAIN),
            "{findings:?}"
        );
        findings.has(mailrs_fraud::RULE_CLAIMS_OUR_DOMAIN)
    }

    /// The message this rule was written for, as production stored it
    /// (`lihao/cur/1789680401.…`, 2026-09-18 06:26 JST).  It went to
    /// Junk on `dmarc=fail` and nothing held it, so it never reached
    /// the review screen.
    ///
    /// Our own receiver stamped it — so it arrived as a stranger — from
    /// a public address, and DMARC failed.  The `spf=pass; dkim=pass`
    /// belong to `pvyo.cn`, the sender's own domain, aligned with
    /// nothing.
    #[test]
    fn the_reported_bec_message_is_held() {
        let raw: &[u8] = b"Authentication-Results: mail.golia.ai;\r\n\
\tspf=pass;\r\n\tdkim=pass;\r\n\tarc=none;\r\n\
\tdmarc=fail reason=\"policy=quarantine\"\r\n\
Received: from mail.golia.ai (162.4.137.23:39822)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
Sender: <mliwzodler@pvyo.cn>\r\n\
From: =?utf-8?B?6b2L6JekIOecnw==?= <aiyhccspbu@golia.jp>\r\n\
To: \"finance@golia.jp\" <finance@golia.jp>\r\n\
Subject: =?utf-8?B?44Ku44Oq44Ki5qCq5byP5Lya56S+IOalreWLmeWkieabtA==?=\r\n\r\nbody\r\n";

        let (_, unauth, private, dmarc) = facts_as_the_sweep_reads_them(raw, "mail.golia.ai");
        assert!(unauth, "our own stamp is on it");
        assert!(!private, "162.4.137.23 is not one of ours");
        assert_eq!(dmarc, "fail");
        assert!(holds_as_the_sweep_would(raw));
    }

    /// And this deployment's own control plane, as production stored it
    /// — `devops@golia.jp`, submitted to the MX from the container
    /// bridge without SMTP AUTH and unsigned, so authentication reads
    /// exactly like the forgery above.
    ///
    /// The first version of the rule held 62 of these.  The user
    /// noticed within the hour: "这不是 review 应该有的啊，这是我们自己
    /// 系统发的".
    #[test]
    fn our_own_control_plane_mail_is_not_held() {
        let raw: &[u8] = b"Authentication-Results: mail.golia.ai;\r\n\
\tspf=softfail;\r\n\tdkim=none;\r\n\tarc=none;\r\n\
\tdmarc=fail reason=\"policy=quarantine\"\r\n\
Received: from mail.golia.ai (172.18.0.1:58646)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
From: devops@golia.jp\r\n\
To: lihao@golia.jp\r\n\
Subject: [devops] expo SDK 58 - latest 57.0.23 -> 57.0.24\r\n\r\nbody\r\n";

        let (_, unauth, private, dmarc) = facts_as_the_sweep_reads_them(raw, "mail.golia.ai");
        assert!(unauth, "it did not authenticate either");
        assert!(private, "172.18.0.1 is this host");
        assert_eq!(dmarc, "fail", "and nothing signed it");
        assert!(
            !holds_as_the_sweep_would(raw),
            "our own control plane was held"
        );
    }

    /// The third shape production has: a public sender at our domain
    /// that DMARC verified.  Aligned means it is us.
    #[test]
    fn a_verified_public_sender_at_our_domain_is_not_held() {
        let raw: &[u8] = b"Authentication-Results: mail.golia.ai;\r\n\
\tspf=pass;\r\n\tdkim=none;\r\n\tdmarc=pass\r\n\
Received: from mail.golia.ai (115.124.28.66:22574)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
From: noreply@golia.jp\r\n\
To: lihao@golia.jp\r\n\
Subject: notice\r\n\r\nbody\r\n";
        assert!(!holds_as_the_sweep_would(raw));
    }
    use super::*;

    const HELD: &[u8] = b"Authentication-Results: mx.golia.jp; spf=pass; dkim=pass; dmarc=pass\r\n\
From: =?UTF-8?B?R09MSUEgSy5LLg==?= <billing@example.invalid>\r\n\
Message-ID: <m1@example.invalid>\r\n\
X-Mailer: 4.28.1.9\r\n\
Subject: Invoice\r\n\
\r\n\
body\r\n";

    /// A conversation the rules no longer hold, that is still
    /// hidden, is let out. Nothing else about the sweep can do this
    /// — every other path either holds or leaves alone.
    #[test]
    fn a_hold_a_rule_no_longer_supports_is_released() {
        assert_eq!(disposition(false, false, true), Disposition::Release);
        assert_eq!(disposition(false, true, true), Disposition::Release);
    }

    /// And the three that must not move.
    #[test]
    fn nothing_else_is_released() {
        assert_eq!(disposition(true, false, true), Disposition::Act);
        assert_eq!(disposition(true, false, false), Disposition::Act);
        assert_eq!(disposition(true, true, false), Disposition::Act);
        assert_eq!(
            disposition(false, false, false),
            Disposition::Leave,
            "mail that was never held is not touched by the release path"
        );
    }

    /// A score that reaches the Junk threshold on its own moves a
    /// conversation nothing holds — and only that: below the
    /// threshold it is left, and a hold still wins.
    #[test]
    fn a_score_alone_at_the_threshold_is_junked_and_nothing_less() {
        assert_eq!(disposition(false, true, false), Disposition::Junk);
        assert_eq!(disposition(false, false, false), Disposition::Leave);
        assert_eq!(disposition(true, true, false), Disposition::Act);
    }

    /// The reported `Hao` message scores into Junk by itself, and a
    /// message that trips nothing does not. Read through the same
    /// function the sweep calls, so the threshold is the one it uses.
    #[test]
    fn the_cut_from_the_address_campaign_reaches_junk_in_the_sweep() {
        let policy = mailrs_fraud::Policy {
            our_domains: vec!["golia.jp".into()],
            ..mailrs_fraud::Policy::default()
        };
        let mut rules = mailrs_fraud_lua::Rules::compile(mailrs_fraud_lua::DEFAULT_SOURCE)
            .expect("shipped bundle compiles");
        let mut judge = |subject: &str| {
            let facts = mailrs_fraud::Facts {
                from: "Erica Flores <ericaf_flores@caredealspark.com>",
                subject,
                to_display: "lihao@golia.jp",
                ..mailrs_fraud::Facts::default()
            };
            let f = rules.classify(&facts, &policy).findings;
            (mailrs_inbound::holds(&f), junks_on_its_own(&f))
        };
        assert_eq!(judge("Hao"), (false, true));
        assert_eq!(judge("Quarterly contracts"), (false, false));
    }

    /// The letter reported on 2026-09-28, as stored: every
    /// authentication check passed, so the offer in the text is the
    /// only thing the sweep can move it on.
    #[test]
    fn the_advance_fee_letter_reaches_junk_in_the_sweep() {
        let raw = b"Authentication-Results: mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass;\r\n\tdmarc=pass\r\n\
From: David Konczol <info@nexforce.in>\r\n\
Subject: RE: PARTNERSHIP PROPOSITION\r\n\
Reply-To: admin@yourmindcoach.in\r\n\
Content-Type: text/plain; charset=UTF-8; format=flowed\r\n\
\r\n\
in the sum of  Ten Million, Two Hundred Thousand United States Dollars \r\n\
only (US$10,200,000.00). This money would be split equally, 50% apiece.\r\n";
        let parsed = mailrs_mime::parse(raw);
        let from = mailrs_inbound::from_header(raw);
        let subject = mailrs_inbound::subject_header(raw);
        let facts = mailrs_fraud::Facts {
            from: &from,
            subject: &subject,
            offers_the_reader_a_sum: mailrs_inbound::body::offers_the_reader_a_sum(&parsed),
            is_bulk: mailrs_inbound::body::is_bulk(raw),
            ..mailrs_fraud::Facts::default()
        };
        let mut rules = mailrs_fraud_lua::Rules::compile(mailrs_fraud_lua::DEFAULT_SOURCE)
            .expect("shipped bundle compiles");
        let f = rules
            .classify(&facts, &mailrs_fraud::Policy::default())
            .findings;
        assert!(f.has("offers-the-reader-a-sum"));
        assert_eq!(
            (mailrs_inbound::holds(&f), junks_on_its_own(&f)),
            (false, true)
        );
    }

    /// Just the `From:`, for a case that is only about the name.
    fn facts(from: &str) -> mailrs_fraud::Facts<'_> {
        mailrs_fraud::Facts {
            from,
            ..mailrs_fraud::Facts::default()
        }
    }

    fn findings() -> mailrs_fraud::Findings {
        let mut f = mailrs_fraud::Findings::new();
        f.push(mailrs_fraud::Finding::new(
            mailrs_fraud::RULE_CLAIMS_OUR_NAME,
            mailrs_fraud::Layer::Identity,
            mailrs_fraud::CLAIMS_OUR_NAME_SCORE,
            "display name claims this organisation",
        ));
        f.push(mailrs_fraud::Finding::new(
            mailrs_fraud::RULE_GENERATED_MAILER,
            mailrs_fraud::Layer::Provenance,
            mailrs_fraud::GENERATED_MAILER_SCORE,
            "X-Mailer is one no mail client writes",
        ));
        f
    }

    /// The receipt, read back. The campaign passed every check, so a
    /// re-scan that reported transport as failing — or as unchecked —
    /// would be telling a different story than the one the receiver
    /// recorded.
    #[test]
    fn a_rescan_reads_the_transport_result_the_receiver_wrote() {
        let v = rescan_verdict(HELD, &findings());
        let t = v.layers.iter().find(|l| l.name == "transport").unwrap();
        assert_eq!(t.outcome, mailrs_inbound::Outcome::Pass);
        assert!(t.detail.contains("SPF pass"), "detail: {}", t.detail);
        assert!(
            v.quarantined,
            "the campaign was not held (score {})",
            v.score
        );
    }

    /// And where there is no receipt it says so. A default that read
    /// as verified would put a tick beside a check that never ran.
    #[test]
    fn without_that_header_transport_is_not_a_pass() {
        let raw = b"From: x <a@b.invalid>\r\nMessage-ID: <m2@b.invalid>\r\n\r\nbody\r\n";
        let v = rescan_verdict(raw, &findings());
        let t = v.layers.iter().find(|l| l.name == "transport").unwrap();
        assert_eq!(t.outcome, mailrs_inbound::Outcome::NotApplicable);
    }

    /// The shape production had on the first sweep: Exchange puts
    /// nothing after `Message-ID:` and folds the value onto the next
    /// line. Read without joining, the id is empty, the verdict is
    /// never stored, and a held conversation cannot say why.
    #[test]
    fn a_folded_header_is_joined() {
        let raw = b"Received: from mail.golia.ai\r\n\tby mail.golia.ai\r\n\
Message-ID:\r\n <SA5PR03MB8426@namprd03.prod.outlook.com>\r\n\
Subject: hi\r\n\r\nbody\r\n";
        assert_eq!(
            header_value(raw, b"message-id:").as_deref(),
            Some("<SA5PR03MB8426@namprd03.prod.outlook.com>")
        );
    }

    /// And the same for the header the scan itself convicts on — a
    /// folded `X-Mailer` read as absent is a check that silently does
    /// not fire.
    ///
    /// Read through `mailrs_inbound`, which is now the only reader:
    /// this file had its own, and two readers of one header is how
    /// the folded case came to work on one path and not the other.
    #[test]
    fn a_folded_x_mailer_is_still_seen() {
        let raw = b"From: a <a@b.invalid>\r\nX-Mailer:\r\n 4.28.1.9\r\n\r\nbody\r\n";
        assert_eq!(
            mailrs_inbound::x_mailer_header(raw).as_deref(),
            Some("4.28.1.9")
        );
    }

    /// Joining stops at the next field, or every header would be one
    /// long string.
    #[test]
    fn joining_stops_at_the_next_header() {
        let raw = b"Subject: one\r\n two\r\nX-Mailer: 9.9.9.9\r\n\r\nbody\r\n";
        assert_eq!(header_value(raw, b"subject:").as_deref(), Some("one two"));
        assert_eq!(
            mailrs_inbound::x_mailer_header(raw).as_deref(),
            Some("9.9.9.9")
        );
    }

    /// A header quoted in the body is not a header.
    #[test]
    fn the_scan_stops_at_the_blank_line() {
        let raw = b"From: x <a@b.invalid>\r\n\r\nX-Mailer: 9.9.9.9\r\n";
        assert_eq!(mailrs_inbound::x_mailer_header(raw), None);
    }

    /// An empty org-name list is a policy that cannot convict on the
    /// name claim, and the sweep's `found` count would come back
    /// looking like an answer. This is the assertion that the two are
    /// distinguishable at all: same message, two policies, two
    /// results.
    ///
    /// It is the defect that shipped — `MAILRS_ORG_NAMES` was set on
    /// the receiver and not on this process, so every sweep run here
    /// saw only the mailer fingerprint.
    #[test]
    fn without_org_names_the_impersonation_check_cannot_fire() {
        let from = "=?UTF-8?B?R09MSUEgSy5LLg==?= <billing@example.invalid>";
        let decoded = mailrs_inbound::from_header(format!("From: {from}\r\n\r\n").as_bytes());

        let configured = mailrs_fraud::Policy {
            account_names: Vec::new(),
            org_names: vec!["GOLIA K.K.".into()],
            our_domains: vec!["golia.jp".into()],
            allowed_domains: Vec::new(),
        };
        let empty = mailrs_fraud::Policy {
            account_names: Vec::new(),
            org_names: Vec::new(),
            our_domains: vec!["golia.jp".into()],
            allowed_domains: Vec::new(),
        };

        assert!(
            mailrs_fraud::scan(&facts(&decoded), &configured)
                .has(mailrs_fraud::RULE_CLAIMS_OUR_NAME),
            "the configured policy did not catch a message claiming to be us"
        );
        assert!(
            !mailrs_fraud::scan(&facts(&decoded), &empty).has(mailrs_fraud::RULE_CLAIMS_OUR_NAME),
            "an empty policy convicted, so this test cannot tell the two apart"
        );
    }

    /// `hold` is a value the route accepts. Without this the action
    /// exists in Rust and not on the wire, which is the shape where a
    /// caller passes `action=hold`, serde rejects it, and the sweep
    /// quietly does the default thing instead.
    #[test]
    fn hold_is_reachable_from_the_query_string() {
        let a: Action = serde_json::from_str("\"hold\"").expect("hold parses");
        assert_eq!(a, Action::Hold);
        assert_eq!(
            serde_json::from_str::<Action>("\"junk\"").unwrap(),
            Action::Junk,
            "the default must still parse"
        );
    }
}
