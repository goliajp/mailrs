//! What a walk's verdicts change against what is stored now.
//!
//! A pack-against-pack diff judges both sides with the same binary, so it
//! cannot see what a new binary changes: the conversations stored today
//! were judged by whichever binary was running when they arrived or were
//! last re-judged. This compares the stored state of each conversation
//! (held or not, and its bucket) with where this walk puts it, which is
//! what the re-judge after an activation will do.

use std::collections::{BTreeMap, BTreeSet};

/// Where a conversation is, as its reader sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Place {
    Held,
    Junk,
    /// Any bucket but Junk: Inbox, Notifications, Promotions.
    Inbox,
}

impl Place {
    fn name(self) -> &'static str {
        match self {
            Place::Held => "held",
            Place::Junk => "junk",
            Place::Inbox => "inbox",
        }
    }
}

/// A conversation's stored state: held, and whether its bucket is Junk.
/// A release returns it to its bucket, so both are kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Position {
    pub(crate) held: bool,
    pub(crate) junk: bool,
}

impl Position {
    pub(crate) fn place(self) -> Place {
        match (self.held, self.junk) {
            (true, _) => Place::Held,
            (false, true) => Place::Junk,
            (false, false) => Place::Inbox,
        }
    }
}

#[derive(Default)]
pub(crate) struct FromStored {
    hold: u64,
    hold_from: BTreeMap<&'static str, u64>,
    release: u64,
    release_to: BTreeMap<&'static str, u64>,
    to_junk: u64,
    out_of_junk: u64,
    to_inbox: u64,
    out_of_inbox: u64,
    out_of_inbox_domains: BTreeSet<String>,
    by_rule: BTreeMap<&'static str, BTreeMap<String, u64>>,
    samples: Vec<serde_json::Value>,
}

/// The moves from `before` to `after`. A hold out of Junk is a hold, not a
/// move out of Junk: the conversation does not reach the inbox.
pub(crate) fn moves(before: Position, after: Position) -> Vec<&'static str> {
    let (b, a) = (before.place(), after.place());
    let mut out = Vec::new();
    if !before.held && after.held {
        out.push("hold");
    }
    if before.held && !after.held {
        out.push("release");
    }
    if a == Place::Junk && b != Place::Junk {
        out.push("to_junk");
    }
    if b == Place::Junk && a == Place::Inbox {
        out.push("out_of_junk");
    }
    if a == Place::Inbox && b != Place::Inbox {
        out.push("to_inbox");
    }
    if b == Place::Inbox && a != Place::Inbox {
        out.push("out_of_inbox");
    }
    out
}

impl FromStored {
    /// One conversation: stored at `before`, put at `after` by `rules`.
    pub(crate) fn record(
        &mut self,
        (before, after): (Position, Position),
        who: (&str, &str),
        domain: &str,
        rules: &[&str],
    ) {
        let moved = moves(before, after);
        if moved.is_empty() {
            return;
        }
        for m in &moved {
            match *m {
                "hold" => {
                    self.hold += 1;
                    *self.hold_from.entry(before.place().name()).or_default() += 1;
                }
                "release" => {
                    self.release += 1;
                    *self.release_to.entry(after.place().name()).or_default() += 1;
                }
                "to_junk" => self.to_junk += 1,
                "out_of_junk" => self.out_of_junk += 1,
                "to_inbox" => self.to_inbox += 1,
                _ => {
                    self.out_of_inbox += 1;
                    let shown = match domain.is_empty() {
                        true => "(no sender domain)",
                        false => domain,
                    };
                    self.out_of_inbox_domains.insert(shown.to_string());
                }
            }
            let counts = self.by_rule.entry(m).or_default();
            for r in rules {
                *counts.entry((*r).to_string()).or_default() += 1;
            }
        }
        if self.samples.len() < 25 {
            self.samples.push(serde_json::json!({
                "user": who.0,
                "thread": who.1,
                "from": before.place().name(),
                "to": after.place().name(),
                "moves": moved,
                "rules": rules,
            }));
        }
    }

    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "would_hold": self.hold,
            "would_hold_from": self.hold_from,
            "would_release": self.release,
            "would_release_to": self.release_to,
            "would_move_to_junk": self.to_junk,
            "would_move_out_of_junk": self.out_of_junk,
            "would_move_to_inbox": self.to_inbox,
            "would_move_out_of_inbox": self.out_of_inbox,
            "out_of_inbox_sender_domains": self.out_of_inbox_domains,
            "by_rule": self.by_rule,
            "samples": self.samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INBOX: Position = Position {
        held: false,
        junk: false,
    };
    const JUNK: Position = Position {
        held: false,
        junk: true,
    };
    const HELD_FROM_JUNK: Position = Position {
        held: true,
        junk: true,
    };
    const HELD_FROM_INBOX: Position = Position {
        held: true,
        junk: false,
    };

    #[test]
    fn a_hold_out_of_junk_is_not_a_move_out_of_junk() {
        assert_eq!(moves(JUNK, HELD_FROM_JUNK), ["hold"]);
        assert_eq!(moves(INBOX, HELD_FROM_INBOX), ["hold", "out_of_inbox"]);
    }

    #[test]
    fn a_release_returns_to_the_bucket_it_left() {
        assert_eq!(moves(HELD_FROM_JUNK, JUNK), ["release", "to_junk"]);
        assert_eq!(moves(HELD_FROM_INBOX, INBOX), ["release", "to_inbox"]);
    }

    #[test]
    fn scoring_into_junk_moves_out_of_the_inbox_and_the_reverse_is_counted() {
        assert_eq!(moves(INBOX, JUNK), ["to_junk", "out_of_inbox"]);
        assert_eq!(moves(JUNK, INBOX), ["out_of_junk", "to_inbox"]);
        assert!(moves(JUNK, JUNK).is_empty());
    }

    #[test]
    fn the_tally_names_the_domains_leaving_the_inbox() {
        let mut t = FromStored::default();
        t.record((INBOX, HELD_FROM_INBOX), ("u", "t"), "x.invalid", &["r1"]);
        t.record((JUNK, HELD_FROM_JUNK), ("u", "t2"), "y.invalid", &["r2"]);
        let j = t.json();
        assert_eq!(j["would_hold"], 2);
        assert_eq!(j["would_hold_from"]["junk"], 1);
        assert_eq!(j["would_move_out_of_junk"], 0);
        assert_eq!(j["would_move_out_of_inbox"], 1);
        assert_eq!(
            j["out_of_inbox_sender_domains"],
            serde_json::json!(["x.invalid"])
        );
        assert_eq!(j["by_rule"]["hold"]["r1"], 1);
    }
}
