//! What was decided about a message, in the shape it is stored.
//!
//! The wave this was built for **passed every authentication check**:
//! all 25 impersonating messages passed SPF, 18 passed DKIM and DMARC.
//! A screen that says "SPF ✓ DKIM ✓ DMARC ✓ — held anyway, because the
//! display name claims to be this company and the mailer is one no
//! client writes" tells the reader something true that no single
//! check can say. That sentence is what this type carries.
//!
//! Written once, at receive time, and never recomputed: the reader has
//! to see what was decided **then**, not what today's rules would
//! decide. [`RULES_VERSION`] is stamped on every verdict so an old one
//! is legible as old rather than as wrong.

use crate::AuthResults;
use crate::context::DmarcPolicy;
use crate::decision::{PipelineInput, SUSPICIOUS_SENDER_SCORE, UNJUSTIFIED_ZERO_WIDTH_SCORE};
use serde::{Deserialize, Serialize};

/// The rule set that produced a verdict.
///
/// Bump when a layer's meaning changes — a new signal, a re-weighted
/// score, a different threshold. Not when a detail string is reworded.
pub const RULES_VERSION: &str = "2026-08-29.1";

/// How one layer came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The layer looked and found nothing wrong.
    Pass,
    /// The layer found something, and it counted towards the score.
    Fail,
    /// The layer had nothing to work with. Distinct from `Pass` on
    /// purpose: "SPF said nothing" and "SPF said yes" are different
    /// facts, and a screen that renders both as a tick is lying.
    NotApplicable,
}

/// One of the four reviews, and what it contributed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Stable identifier — `transport`, `identity`, `provenance`,
    /// `content`. The screen's label is the screen's business.
    pub name: String,
    /// Pass, fail, or nothing to go on.
    pub outcome: Outcome,
    /// This layer's contribution to the total. Zero on a pass.
    pub score: f64,
    /// What it actually saw, in the words a person needs when asking
    /// why their mail is here.
    pub detail: String,
}

/// The whole finding about one message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FraudVerdict {
    /// The rule set in force when this was decided.
    pub rules_version: String,
    /// Sum of the layers.
    ///
    /// Reported, not decided on: what holds a conversation is
    /// [`holds`], and a threshold beside a score that does not reach
    /// it would read as a contradiction on a screen that is showing
    /// the conversation as held.
    pub score: f64,
    /// Whether the message was held.
    pub quarantined: bool,
    /// The four reviews, always all four and always in this order, so
    /// a reader comparing two verdicts is comparing like with like.
    pub layers: Vec<Layer>,
}

/// Whether these findings take a conversation out of every list.
///
/// **Any fraud finding at all**, and the one definition every reader
/// shares. It was a score threshold of 8.0 until production disagreed
/// with itself: the re-scan held on any finding while the verdict it
/// stored said `quarantined: false`, so 43 of 51 held conversations
/// carried a verdict claiming they should not be held — and the
/// screen, which renders "Held" only on that field, would have told
/// the reader those were merely examined.
///
/// The threshold was a prediction — "neither signal alone should take
/// a conversation out of every list" — and the measurement is the
/// other way. Over 35,799 production messages the mailer fingerprint
/// fired 29 times and was right 29 times; the name claim fired 12
/// times and was right 9, and the three it was wrong about were
/// Slack, which is now on the allow-list the check consults. Both
/// convict alone.
///
/// A message that passes SPF, DKIM and DMARC and carries an X-Mailer
/// no client writes is exactly the case this feature exists for, and
/// scoring it 5.0 against an 8.0 bar would let it through.
#[must_use]
pub fn holds(findings: &mailrs_fraud::Findings) -> bool {
    findings.hold_worthy()
}

/// A blank slate: nothing checked, nothing claimed.
///
/// Every authentication token is `none`, so a caller that fills in
/// only what it knows gets `NotApplicable` for the rest rather than a
/// pass. That is the point — the re-scan over old mail knows the
/// identity and provenance layers and does not know what transport
/// said, and a default that read as "verified" would put a tick beside
/// a check that never ran.
#[must_use]
pub fn unexamined() -> PipelineInput {
    PipelineInput {
        greylisted: false,
        auth: AuthResults {
            spf: "none".into(),
            dkim: "none".into(),
            arc: "none".into(),
            dmarc: "none".into(),
            dmarc_policy: DmarcPolicy::None,
        },
        virus_found: None,
        content_score: 0.0,
        matched_rules: Vec::new(),
        ptr_score: 0.0,
        ai_score: 0.0,
        deception: mailrs_textguard::Deception::default(),
        fraud: mailrs_fraud::Findings::default(),
        spam_threshold: 5.0,
        hostname: String::new(),
        from_addr: String::new(),
        header_from_addr: String::new(),
        recipient_whitelist: std::collections::HashSet::new(),
        recipient_blacklist: std::collections::HashSet::new(),
        local_domains: std::collections::HashSet::new(),
    }
}

/// Assemble the four layers from what the pipeline gathered.
///
/// Pure: it reads the same inputs the delivery decision read, so the
/// two cannot disagree about what was seen — only about what to do
/// with it.
#[must_use]
pub fn assess(input: &PipelineInput) -> FraudVerdict {
    let layers = vec![
        transport(&input.auth),
        from_findings(input, mailrs_fraud::Layer::Identity),
        from_findings(input, mailrs_fraud::Layer::Provenance),
        content(input),
    ];
    let score = layers.iter().map(|l| l.score).sum();
    FraudVerdict {
        rules_version: RULES_VERSION.to_string(),
        score,
        quarantined: holds(&input.fraud),
        layers,
    }
}

/// Layer 1 — did it come from where it says it did?
///
/// Contributes nothing to the score. Authentication is what the fraud
/// in question already passes, and a layer that scored it would be
/// scoring the wrong thing; it is here because the reader needs to see
/// that it was checked and came out clean.
fn transport(auth: &AuthResults) -> Layer {
    let detail = format!(
        "SPF {}, DKIM {}, DMARC {} (policy {})",
        auth.spf,
        auth.dkim,
        auth.dmarc,
        match auth.dmarc_policy {
            DmarcPolicy::Reject => "reject",
            DmarcPolicy::Quarantine => "quarantine",
            DmarcPolicy::None => "none",
            DmarcPolicy::Pass => "n/a (verified)",
        }
    );
    let outcome = match auth.dmarc.as_str() {
        "pass" => Outcome::Pass,
        "fail" => Outcome::Fail,
        _ => Outcome::NotApplicable,
    };
    Layer {
        name: "transport".to_string(),
        outcome,
        score: 0.0,
        detail,
    }
}

/// Layers 2 and 3 — and any other a rule files itself under.
///
/// **Grouped, not enumerated.** These were two functions of hard-coded
/// `if`s, one per check, so every new rule meant editing the renderer
/// as well as writing the rule. A finding now carries the review it
/// speaks for and this collects them, which is what makes "many
/// rules" a list rather than a diff across seven files.
///
/// The host's own non-rule signals — zero-width padding, sender
/// trust — are folded in here rather than made into rules, because
/// they come from elsewhere (`mailrs_textguard`, the auth fold) and
/// pretending otherwise would put two owners on one fact.
fn from_findings(input: &PipelineInput, layer: mailrs_fraud::Layer) -> Layer {
    let mut score = 0.0;
    let mut found: Vec<String> = Vec::new();
    for f in input.fraud.in_layer(layer) {
        score += f.score;
        found.push(f.detail.clone());
    }
    if layer == mailrs_fraud::Layer::Identity {
        // The host's own signals, and they are scored rather than
        // held: `mailrs_textguard` measured one legitimate message in
        // forty carrying a zero-width character in its identifying
        // text, and a suspicious sender-trust verdict on its own was
        // never meant to hide mail.
        //
        // They are shown, because the reader asking "why is this
        // here" wants them. Whether they *held* it is the finding's
        // own `holds`, which is why the two can no longer disagree.
        if input.deception.unjustified_zero_width {
            score += UNJUSTIFIED_ZERO_WIDTH_SCORE;
            found.push("zero-width padding in the identifying text (scored, not held)".into());
        }
        if input.auth.sender_trust_with(input.deception)
            == crate::auth_header::SenderTrust::Suspicious
        {
            score += SUSPICIOUS_SENDER_SCORE;
            found.push("sender trust: suspicious (scored, not held)".into());
        }
    }
    let clean = match layer {
        mailrs_fraud::Layer::Identity => "the name and address agree",
        mailrs_fraud::Layer::Provenance => "nothing unusual in the headers",
        _ => "nothing found",
    };
    layer_from(layer.as_str(), score, found, clean)
}

/// Layer 4 — does it read like the fraud it is?
fn content(input: &PipelineInput) -> Layer {
    let score = input.content_score + input.ai_score;
    let detail = if input.matched_rules.is_empty() {
        format!(
            "content {:.1}, classifier {:.1}",
            input.content_score, input.ai_score
        )
    } else {
        format!(
            "content {:.1}, classifier {:.1} ({})",
            input.content_score,
            input.ai_score,
            input.matched_rules.join(", ")
        )
    };
    Layer {
        name: "content".to_string(),
        outcome: outcome_for(score),
        score,
        detail,
    }
}

fn outcome_for(score: f64) -> Outcome {
    if score > 0.0 {
        Outcome::Fail
    } else {
        Outcome::Pass
    }
}

fn layer_from(name: &str, score: f64, found: Vec<String>, clean: &str) -> Layer {
    let detail = if found.is_empty() {
        clean.to_string()
    } else {
        found.join("; ")
    };
    Layer {
        name: name.to_string(),
        outcome: outcome_for(score),
        score,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::DmarcPolicy;

    fn input() -> PipelineInput {
        PipelineInput {
            greylisted: false,
            auth: AuthResults {
                spf: "pass".into(),
                dkim: "pass".into(),
                arc: "none".into(),
                dmarc: "pass".into(),
                dmarc_policy: DmarcPolicy::Pass,
            },
            virus_found: None,
            content_score: 0.0,
            matched_rules: vec![],
            ptr_score: 0.0,
            ai_score: 0.0,
            deception: mailrs_textguard::Deception::default(),
            fraud: mailrs_fraud::Findings::default(),
            spam_threshold: 5.0,
            hostname: "mx.example.com".into(),
            from_addr: String::new(),
            header_from_addr: String::new(),
            recipient_whitelist: std::collections::HashSet::new(),
            recipient_blacklist: std::collections::HashSet::new(),
            local_domains: std::collections::HashSet::new(),
        }
    }

    fn layer<'a>(v: &'a FraudVerdict, name: &str) -> &'a Layer {
        v.layers
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no {name} layer in {:?}", v.layers))
    }

    /// The four are always four, always in order, always named the
    /// same. A screen comparing two verdicts is comparing like with
    /// like only if this holds.
    #[test]
    fn there_are_always_four_layers_in_one_order() {
        let v = assess(&input());
        let names: Vec<&str> = v.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["transport", "identity", "provenance", "content"]);
    }

    /// Clean mail is held by nothing, and every layer says so.
    #[test]
    fn authenticated_ordinary_mail_is_not_held() {
        let v = assess(&input());
        assert_eq!(v.score, 0.0);
        assert!(!v.quarantined);
        assert_eq!(layer(&v, "transport").outcome, Outcome::Pass);
        assert_eq!(layer(&v, "identity").outcome, Outcome::Pass);
        assert_eq!(layer(&v, "provenance").outcome, Outcome::Pass);
        assert_eq!(layer(&v, "content").outcome, Outcome::Pass);
    }

    /// The case this whole feature exists for, and the reason the
    /// screen is worth a rail entry: the mail is *authentic* and still
    /// fraudulent. Transport passes; identity and provenance convict.
    #[test]
    fn the_campaign_passes_transport_and_is_held_anyway() {
        let mut i = input();
        i.fraud = mailrs_fraud::findings_for(&[mailrs_fraud::RULE_CLAIMS_OUR_NAME]);
        i.fraud.extend(mailrs_fraud::findings_for(&[
            mailrs_fraud::RULE_GENERATED_MAILER,
        ]));
        let v = assess(&i);

        assert_eq!(
            layer(&v, "transport").outcome,
            Outcome::Pass,
            "the wave passed SPF, DKIM and DMARC; a verdict that says otherwise is not this wave"
        );
        assert_eq!(layer(&v, "identity").outcome, Outcome::Fail);
        assert_eq!(layer(&v, "provenance").outcome, Outcome::Fail);
        assert!(
            v.quarantined,
            "the campaign was not held (score {})",
            v.score
        );
    }

    /// **Either signal alone holds.** This asserted the opposite until
    /// 2026-08-29, when production held 51 conversations and stored 43
    /// verdicts saying they should not have been — the re-scan held on
    /// any finding while this said 8.0. The measurement settles it:
    /// the mailer fingerprint fired 29 times and was right 29 times,
    /// and the name claim was right 9 of 12 with the three misses now
    /// on the allow-list.
    #[test]
    fn either_signal_alone_holds_a_conversation() {
        for rule in [
            mailrs_fraud::RULE_CLAIMS_OUR_NAME,
            mailrs_fraud::RULE_GENERATED_MAILER,
            mailrs_fraud::RULE_IMPERSONATES_BRAND,
        ] {
            let mut i = input();
            i.fraud = mailrs_fraud::findings_for(&[rule]);
            let v = assess(&i);
            assert!(
                v.quarantined,
                "a fraud finding did not hold ({:.1})",
                v.score
            );
        }
    }

    /// The mail the user asked about: `iCloud+` from
    /// `zkxfp@zkxfp.zctxiot.com`, asking them to confirm a payment
    /// method. It claims a company they have an account with, not
    /// this one, so `claims_our_name` had nothing to say and it
    /// reached the inbox with only a "Suspicious sender" badge.
    ///
    /// The identity layer has to name it, and `holds` has to hold it
    /// — a signal that stops at the score is a signal nobody acts on.
    #[test]
    fn a_brand_impersonation_is_named_and_held() {
        let mut i = input();
        i.fraud = mailrs_fraud::findings_for(&[mailrs_fraud::RULE_IMPERSONATES_BRAND]);
        let v = assess(&i);

        let identity = v.layers.iter().find(|l| l.name == "identity").unwrap();
        assert_eq!(identity.outcome, Outcome::Fail);
        assert!(
            identity.detail.contains("claims a company"),
            "the layer did not say what it found: {}",
            identity.detail
        );
        assert!(v.quarantined, "a brand impersonation was not held");
    }

    /// And the negative, so the assertion above cannot pass on a
    /// verdict that holds everything: clean mail is not held.
    #[test]
    fn a_message_with_no_finding_is_not_held() {
        assert!(!assess(&input()).quarantined);
        // Content score alone is a Junk decision, not a hold: the
        // hold is about who the sender is, not how the message reads.
        let mut i = input();
        i.content_score = 9.9;
        assert!(
            !assess(&i).quarantined,
            "content score alone held a conversation"
        );
    }

    /// One definition, and the sweep reads the same one. A second
    /// copy of "what counts as held" is what produced 43 verdicts
    /// that disagreed with the hold they were written for.
    #[test]
    fn holds_is_the_only_definition() {
        let none = mailrs_fraud::Findings::new();
        assert!(!holds(&none));
        let one = mailrs_fraud::findings_for(&[mailrs_fraud::RULE_GENERATED_MAILER]);
        assert!(holds(&one));

        let mut i = input();
        i.fraud = one.clone();
        assert_eq!(assess(&i).quarantined, holds(&one));
    }

    /// Transport contributes nothing to the score — on purpose, since
    /// the fraud passes it — but it must still be able to report a
    /// failure. A layer that can only say one thing is not a check.
    #[test]
    fn transport_reports_a_failure_it_does_not_score() {
        let mut i = input();
        i.auth.dmarc = "fail".into();
        i.auth.dmarc_policy = DmarcPolicy::Reject;
        let t = layer(&assess(&i), "transport").clone();
        assert_eq!(t.outcome, Outcome::Fail);
        assert_eq!(t.score, 0.0);
        assert!(t.detail.contains("DMARC fail"), "detail: {}", t.detail);
    }

    /// "Nothing was checked" is not "everything was fine". A screen
    /// that renders both as a tick is telling the reader something
    /// untrue about mail nobody vouched for.
    #[test]
    fn unchecked_transport_is_not_a_pass() {
        let mut i = input();
        i.auth.dmarc = "none".into();
        i.auth.dmarc_policy = DmarcPolicy::None;
        assert_eq!(
            layer(&assess(&i), "transport").outcome,
            Outcome::NotApplicable
        );
    }

    /// The verdict is what gets stored, so it has to survive the trip.
    #[test]
    fn it_round_trips_through_json() {
        let mut i = input();
        i.fraud = mailrs_fraud::findings_for(&[mailrs_fraud::RULE_CLAIMS_OUR_NAME]);
        i.content_score = 1.5;
        i.matched_rules = vec!["urgent_wire".into()];
        let before = assess(&i);
        let json = serde_json::to_string(&before).expect("serialize");
        let after: FraudVerdict = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(before, after);
        assert!(json.contains(RULES_VERSION), "the stamp did not survive");
    }

    /// Every layer's score is accounted for in the total. A layer
    /// whose contribution is dropped is a number the screen shows and
    /// the decision never used.
    #[test]
    fn the_total_is_the_sum_of_the_parts() {
        let mut i = input();
        i.fraud = mailrs_fraud::findings_for(&[mailrs_fraud::RULE_CLAIMS_OUR_NAME]);
        i.fraud.extend(mailrs_fraud::findings_for(&[
            mailrs_fraud::RULE_GENERATED_MAILER,
        ]));
        i.content_score = 2.0;
        i.ai_score = 1.0;
        let v = assess(&i);
        let summed: f64 = v.layers.iter().map(|l| l.score).sum();
        assert!((v.score - summed).abs() < f64::EPSILON);
    }
}
