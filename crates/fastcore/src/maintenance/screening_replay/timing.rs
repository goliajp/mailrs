//! Judging a message by what was known when it arrived.
//!
//! The replay builds the reputation and hash sets from today's
//! observations, so a sender listed after a message arrived counts against
//! it. Such a hit is not a detection: nothing could have caught the message
//! then. A hit is `after_the_fact` when it cites observations and every one
//! it cites was recorded after the message arrived. A hit that cites none
//! (an attachment's structure) or cites at least one recorded in time is a
//! detection.

use std::collections::HashMap;

/// When each observation was recorded, by id.
pub(super) struct ObservedAt(HashMap<String, i64>);

impl ObservedAt {
    pub(super) fn new(
        obs: &[mailrs_reputation::Observation],
        hashes: &[mailrs_attachment_scan::HashObservation],
    ) -> Self {
        let mut m = HashMap::new();
        for o in obs {
            m.insert(o.id.clone(), o.at);
        }
        for h in hashes {
            m.insert(h.id.clone(), h.at);
        }
        ObservedAt(m)
    }

    /// Whether every observation `evidence` cites came after `arrived`.
    /// Each evidence line starts with the id of the observation it cites
    /// (`helpers.lua`, `attachments.lua`); a line that names none is not
    /// an observation and says nothing about time.
    pub(super) fn after_the_fact(&self, evidence: &[String], arrived: i64) -> bool {
        let cited: Vec<i64> = evidence
            .iter()
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(|id| self.0.get(id).copied())
            .collect();
        !cited.is_empty() && cited.iter().all(|at| *at > arrived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(id: &str, at: i64) -> mailrs_reputation::Observation {
        mailrs_reputation::Observation {
            id: id.into(),
            at,
            source: mailrs_reputation::Source::Manual,
            kind: mailrs_reputation::Kind::Ip,
            value: "192.0.2.1".into(),
            stance: mailrs_reputation::Stance::Deny,
            retracts: None,
            reason: "test".into(),
            actor: None,
        }
    }

    #[test]
    fn a_hit_only_later_observations_explain_is_after_the_fact() {
        let at = ObservedAt::new(&[obs("r1", 200), obs("r2", 50)], &[]);
        let line = |id: &str| vec![format!("{id} (manual, 3 days): why")];
        assert!(at.after_the_fact(&line("r1"), 100));
        assert!(
            !at.after_the_fact(&line("r2"), 100),
            "known before it arrived"
        );
        let both = vec![line("r1")[0].clone(), line("r2")[0].clone()];
        assert!(!at.after_the_fact(&both, 100), "one in time is enough");
        assert!(
            !at.after_the_fact(&["invoice.pdf: script".to_string()], 100),
            "a structural hit cites no observation"
        );
    }
}
