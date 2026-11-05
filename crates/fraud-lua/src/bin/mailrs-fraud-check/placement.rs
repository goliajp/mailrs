//! Where each bundle would file a message, and what switching to the
//! candidate moves between the inbox, Junk and the held list.
//!
//! The decision is the re-judge sweep's: a hold-level finding holds;
//! otherwise the fraud score alone against the Junk threshold sends it
//! to Junk, unless the reader marked the sender not junk. What the sweep
//! or the receive path reads that this cannot is listed in the report
//! under `placement_not_modelled`, so a zero here is not read as "nothing
//! else would move".

use mailrs_fraud::Findings;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Held,
    Junk,
    Inbox,
}

impl Place {
    pub fn as_str(self) -> &'static str {
        match self {
            Place::Held => "held",
            Place::Junk => "junk",
            Place::Inbox => "inbox",
        }
    }
}

pub fn place(f: &Findings, whitelisted: bool) -> Place {
    if mailrs_inbound::holds(f) {
        return Place::Held;
    }
    if f.score() >= mailrs_inbound::DEFAULT_SPAM_THRESHOLD && !whitelisted {
        return Place::Junk;
    }
    Place::Inbox
}

/// What the receive path adds to the score, or decides before it, that
/// a corpus read cannot reproduce; and how the re-judge applies a move.
const NOT_MODELLED: &[&str] = &[
    "content score (spam rules on the body at receive time)",
    "PTR score",
    "Bayes score (network kevy, trained per recipient at receive time)",
    "suspicious-sender and zero-width padding scores",
    "recipient blacklist, operator allow list, DMARC policy",
    "current folder: the re-judge never takes a conversation out of Junk by score, and a released conversation returns to the folder it was in before the hold",
    "conversations: the re-judge judges a conversation's newest message, this counts every message",
];

#[derive(Default)]
pub struct Moves {
    to_junk: u64,
    out_of_junk: u64,
    to_inbox: u64,
    out_of_inbox: u64,
    out_of_inbox_domains: BTreeSet<String>,
    /// per move, the rules each moved message gained (`+rule`) or lost
    /// (`-rule`)
    by_rule: BTreeMap<&'static str, BTreeMap<String, u64>>,
}

impl Moves {
    pub fn record(
        &mut self,
        (before, after): (Place, Place),
        domain: &str,
        old: &Findings,
        new: &Findings,
    ) {
        if before == after {
            return;
        }
        let mut moves = Vec::new();
        if after == Place::Junk {
            self.to_junk += 1;
            moves.push("to_junk");
        }
        if before == Place::Junk {
            self.out_of_junk += 1;
            moves.push("out_of_junk");
        }
        if after == Place::Inbox {
            self.to_inbox += 1;
            moves.push("to_inbox");
        }
        if before == Place::Inbox {
            self.out_of_inbox += 1;
            moves.push("out_of_inbox");
            let shown = match domain.is_empty() {
                true => "(no sender domain)",
                false => domain,
            };
            self.out_of_inbox_domains.insert(shown.to_string());
        }
        let was: BTreeSet<&str> = old.rules().into_iter().collect();
        let is: BTreeSet<&str> = new.rules().into_iter().collect();
        for m in moves {
            let counts = self.by_rule.entry(m).or_default();
            for r in is.difference(&was) {
                *counts.entry(format!("+{r}")).or_default() += 1;
            }
            for r in was.difference(&is) {
                *counts.entry(format!("-{r}")).or_default() += 1;
            }
        }
    }

    /// The report's placement fields. `whitelist_checked`: whether the
    /// readers' not-junk lists were read from the shared store.
    pub fn report(&self, report: &mut serde_json::Value, whitelist_checked: bool) {
        let mut not_modelled: Vec<&str> = NOT_MODELLED.to_vec();
        if !whitelist_checked {
            not_modelled.push("recipient whitelist (needs MAILRS_KEVY_URL)");
        }
        report["would_move_to_junk"] = self.to_junk.into();
        report["would_move_out_of_junk"] = self.out_of_junk.into();
        report["would_move_to_inbox"] = self.to_inbox.into();
        report["would_move_out_of_inbox"] = self.out_of_inbox.into();
        report["out_of_inbox_sender_domains"] = serde_json::json!(self.out_of_inbox_domains);
        report["placement_moves_by_rule"] = serde_json::json!(self.by_rule);
        report["placement_model"] = serde_json::json!(format!(
            "held if a hold-level rule fires, else Junk if the fraud score alone is >= {} and the reader has not marked the sender not junk, else inbox (the re-judge sweep's decision)",
            mailrs_inbound::DEFAULT_SPAM_THRESHOLD
        ));
        report["whitelist_checked"] = whitelist_checked.into();
        report["placement_not_modelled"] = serde_json::json!(not_modelled);
    }
}

/// The lines `deploy-fraud-rules.sh` prints from a saved report.
pub fn summary(report: &serde_json::Value) -> Vec<String> {
    let n = |k: &str| report[k].to_string();
    let mut lines = vec![
        format!(
            "holds: {} added, {} removed, {} held by the candidate",
            n("added_holds"),
            n("removed_holds"),
            n("candidate_holds")
        ),
        format!("would move to Junk: {}", n("would_move_to_junk")),
        format!("would move out of Junk: {}", n("would_move_out_of_junk")),
        format!("would move to inbox: {}", n("would_move_to_inbox")),
        format!("would move out of inbox: {}", n("would_move_out_of_inbox")),
    ];
    let domains: Vec<&str> = report["out_of_inbox_sender_domains"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d.as_str())
        .collect();
    if !domains.is_empty() {
        lines.push(format!(
            "out of inbox, sender domains: {}",
            domains.join(", ")
        ));
    }
    lines.push(format!("whitelist checked: {}", n("whitelist_checked")));
    for m in report["placement_not_modelled"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d.as_str())
    {
        lines.push(format!("not modelled: {m}"));
    }
    lines
}
