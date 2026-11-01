//! Why a delivered message was filed where it was, for one recipient, and
//! the record a core answers "why is this here" with.
//!
//! The receiver's verdict is stored once per message and is the same for
//! every recipient. Where each recipient's copy was filed is not: one has
//! a sender rule, another a filter. So that part is stored per recipient
//! and joined to the verdict when read, by these two functions, which
//! both cores call.

use mailrs_inbound::reason::{JUNK_YOUR_FILTER, Reason};
use mailrs_inbound::verdict::FraudVerdict;

/// What to store for this recipient: `placement` from the arrival, less
/// the filter's Junk when the receiver had already put it there.
pub fn recipient_placement(receiver: Option<&str>, placement: &[Reason]) -> Vec<Reason> {
    let receiver_junked = receiver
        .and_then(|json| serde_json::from_str::<FraudVerdict>(json).ok())
        .is_some_and(|v| v.placement.iter().any(|r| r.code.starts_with("junk-")));
    placement
        .iter()
        // the receiver put it in Junk first; the filter only agreed
        .filter(|r| !(receiver_junked && r.code == JUNK_YOUR_FILTER))
        .cloned()
        .collect()
}

/// The record one recipient reads: the stored verdict with their own
/// placement added, or their placement alone when nobody suspected the
/// message. `None` when there is neither. A stored verdict that does not
/// parse is returned as it is.
pub fn joined(
    verdict: Option<serde_json::Value>,
    placement: Vec<Reason>,
) -> Option<serde_json::Value> {
    if placement.is_empty() {
        return verdict;
    }
    let Some(stored) = verdict else {
        return serde_json::to_value(FraudVerdict::placement_only(placement)).ok();
    };
    match serde_json::from_value::<FraudVerdict>(stored.clone()) {
        Ok(mut v) => {
            v.placement.extend(placement);
            serde_json::to_value(v).ok()
        }
        Err(_) => Some(stored),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_inbound::reason::{JUNK_SCORE, SORTED_BY_CLASSIFIER, SORTED_BY_YOUR_RULE};

    fn codes(v: &serde_json::Value) -> Vec<&str> {
        v["placement"]
            .as_array()
            .map(|a| a.iter().filter_map(|r| r["code"].as_str()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn nothing_found_and_nothing_sorted_reads_as_nothing() {
        assert_eq!(joined(None, Vec::new()), None);
    }

    #[test]
    fn sorted_mail_nobody_suspected_reads_as_its_sorting() {
        let r = Reason::new(SORTED_BY_CLASSIFIER).with("class", "promotion");
        let v = joined(None, vec![r]).expect("a record");
        assert_eq!(v["layers"], serde_json::json!([]));
        assert_eq!(v["quarantined"], false);
        assert_eq!(codes(&v), ["sorted-by-classifier"]);
    }

    #[test]
    fn the_receivers_junk_reason_outranks_the_filter_that_agreed() {
        let receiver = FraudVerdict::placement_only(vec![Reason::new(JUNK_SCORE)]);
        let json = serde_json::to_string(&receiver).unwrap();
        let mine = recipient_placement(Some(&json), &[Reason::new(JUNK_YOUR_FILTER)]);
        assert!(mine.is_empty(), "{mine:?}");
        // and without the receiver's reason, the filter is named
        let alone = recipient_placement(None, &[Reason::new(JUNK_YOUR_FILTER)]);
        assert_eq!(alone, [Reason::new(JUNK_YOUR_FILTER)]);
    }

    /// Two recipients of one message read one verdict and their own
    /// filing each.
    #[test]
    fn each_recipient_reads_their_own_filing_beside_the_shared_verdict() {
        let mut held = FraudVerdict::placement_only(vec![Reason::new(JUNK_SCORE)]);
        held.quarantined = true;
        let shared = serde_json::to_value(&held).unwrap();
        let alice = joined(Some(shared.clone()), vec![Reason::new(SORTED_BY_YOUR_RULE)]).unwrap();
        let bob = joined(Some(shared), Vec::new()).unwrap();
        assert_eq!(codes(&alice), ["junk-score", "sorted-by-your-rule"]);
        assert_eq!(codes(&bob), ["junk-score"]);
        assert_eq!(alice["quarantined"], true);
    }

    #[test]
    fn a_stored_verdict_that_does_not_parse_is_read_as_it_is() {
        let odd = serde_json::json!({"score": "not a number"});
        let r = Reason::new(SORTED_BY_CLASSIFIER);
        assert_eq!(joined(Some(odd.clone()), vec![r]), Some(odd));
    }
}
