//! Which Junk conversations the correction may move: the pure half.

use mailrs_inbound::reason::JUNK_SCORE;
use mailrs_inbound::verdict::FraudVerdict;

/// Why a conversation stays where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stay {
    /// The fraud rules hold it.
    Held,
    /// The reader filed it as Junk.
    ReaderJunked,
    /// Its newest message is older than the window.
    OutOfWindow,
    /// A message's file is not on disk.
    NoFile,
    /// A message's sender is not `verified`.
    NotVerified,
    /// A message did not pass DMARC.
    NotDmarcPass,
    /// A message carries neither `List-Unsubscribe` nor `List-Id`.
    NotListMail,
    /// A stored verdict does not parse.
    UnreadableVerdict,
    /// A stored verdict found something, or was not the score's.
    NotTheScore,
    /// The score would have junked it without the classifier.
    NotTheClassifier,
    /// A fraud rule scored it.
    FraudScored,
    /// Triage has no bucket for it.
    NoBucket,
}

impl Stay {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Stay::Held => "held",
            Stay::ReaderJunked => "reader_junked",
            Stay::OutOfWindow => "out_of_window",
            Stay::NoFile => "no_file",
            Stay::NotVerified => "not_verified",
            Stay::NotDmarcPass => "not_dmarc_pass",
            Stay::NotListMail => "not_list_mail",
            Stay::UnreadableVerdict => "unreadable_verdict",
            Stay::NotTheScore => "not_the_score",
            Stay::NotTheClassifier => "not_the_classifier",
            Stay::FraudScored => "fraud_scored",
            Stay::NoBucket => "no_bucket",
        }
    }
}

/// What one message of the conversation shows.
pub(crate) struct Seen<'a> {
    /// The stored `sender_trust`.
    pub sender_trust: &'a str,
    /// The raw message, as stored.
    pub raw: &'a [u8],
    /// The receiver's stored verdict, as JSON.
    pub verdict: Option<&'a str>,
}

/// The message is authenticated list mail that only the score put in Junk,
/// and only because of the classifier.
pub(crate) fn judge(m: &Seen<'_>) -> Result<(), Stay> {
    if m.sender_trust != "verified" {
        return Err(Stay::NotVerified);
    }
    if mailrs_inbound::identity::auth_results_tokens(m.raw).2 != "pass" {
        return Err(Stay::NotDmarcPass);
    }
    if !mailrs_inbound::list_mail::has_list_headers(m.raw) {
        return Err(Stay::NotListMail);
    }
    match m.verdict {
        None => Ok(()),
        Some(json) => verdict(json),
    }
}

/// A stored verdict lets the message move only when it is the score's Junk
/// and nothing else: no layer found anything, no fraud rule scored, and
/// without the classifier the score stays under the threshold.
fn verdict(json: &str) -> Result<(), Stay> {
    let Ok(v) = serde_json::from_str::<FraudVerdict>(json) else {
        return Err(Stay::UnreadableVerdict);
    };
    if v.quarantined {
        return Err(Stay::Held);
    }
    // the content layer's line is the classifier and the content scan,
    // which the score below accounts for; any other line is a finding
    let found = v.layers.iter().any(|l| {
        l.reasons.iter().any(|r| {
            r.score.is_some_and(|s| s > 0.0) && r.code != mailrs_inbound::reason::CONTENT_SCORE
        })
    });
    if found {
        return Err(Stay::FraudScored);
    }
    let junk: Vec<_> = v
        .placement
        .iter()
        .filter(|r| r.code.starts_with("junk-"))
        .collect();
    let [only] = junk.as_slice() else {
        return Err(Stay::NotTheScore);
    };
    if only.code != JUNK_SCORE {
        return Err(Stay::NotTheScore);
    }
    let num = |k: &str| only.params.get(k).and_then(|v| v.parse::<f64>().ok());
    let (Some(score), Some(classifier), Some(threshold)) =
        (num("score"), num("classifier"), num("threshold"))
    else {
        return Err(Stay::UnreadableVerdict);
    };
    if num("fraud").is_some_and(|f| f > 0.0) {
        return Err(Stay::FraudScored);
    }
    match score - classifier < threshold {
        true => Ok(()),
        false => Err(Stay::NotTheClassifier),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEWSLETTER: &[u8] =
        b"Authentication-Results: mx.test; spf=pass; dkim=pass; dmarc=pass\r\n\
From: News <news@shop.example>\r\nList-Unsubscribe: <https://shop.example/u>\r\n\
Subject: picks\r\n\r\nhello\r\n";

    fn seen(verdict: Option<&str>) -> Seen<'_> {
        Seen {
            sender_trust: "verified",
            raw: NEWSLETTER,
            verdict,
        }
    }

    fn score_verdict(score: f64, classifier: f64, fraud: f64) -> String {
        let why = mailrs_inbound::Reason::new(JUNK_SCORE)
            .with_score("score", score)
            .with_score("threshold", 5.0)
            .with_score("content", score - classifier - fraud)
            .with_score("classifier", classifier)
            .with_score("fraud", fraud);
        serde_json::to_string(&FraudVerdict::placement_only(vec![why])).unwrap()
    }

    #[test]
    fn authenticated_list_mail_with_no_verdict_moves() {
        assert_eq!(judge(&seen(None)), Ok(()));
    }

    #[test]
    fn the_classifier_decided_so_it_moves() {
        let v = score_verdict(6.0, 4.0, 0.0);
        assert_eq!(judge(&seen(Some(&v))), Ok(()));
    }

    #[test]
    fn the_content_alone_junked_it_so_it_stays() {
        let v = score_verdict(9.0, 4.0, 0.0);
        assert_eq!(judge(&seen(Some(&v))), Err(Stay::NotTheClassifier));
    }

    #[test]
    fn a_fraud_score_keeps_it() {
        let v = score_verdict(6.0, 4.0, 1.0);
        assert_eq!(judge(&seen(Some(&v))), Err(Stay::FraudScored));
    }

    #[test]
    fn another_junk_reason_keeps_it() {
        let why = mailrs_inbound::Reason::new(mailrs_inbound::reason::JUNK_YOUR_BLOCKLIST);
        let v = serde_json::to_string(&FraudVerdict::placement_only(vec![why])).unwrap();
        assert_eq!(judge(&seen(Some(&v))), Err(Stay::NotTheScore));
    }

    #[test]
    fn unauthenticated_or_unlisted_mail_stays() {
        let mut m = seen(None);
        m.sender_trust = "unverified";
        assert_eq!(judge(&m), Err(Stay::NotVerified));
        let failing = b"Authentication-Results: mx.test; dmarc=none\r\nList-Id: <a.b>\r\n\r\nx";
        let m = Seen {
            sender_trust: "verified",
            raw: failing,
            verdict: None,
        };
        assert_eq!(judge(&m), Err(Stay::NotDmarcPass));
        let unlisted = b"Authentication-Results: mx.test; dmarc=pass\r\nFrom: a@b.example\r\n\r\nx";
        let m = Seen {
            sender_trust: "verified",
            raw: unlisted,
            verdict: None,
        };
        assert_eq!(judge(&m), Err(Stay::NotListMail));
    }
}
