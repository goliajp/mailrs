//! Rules: a saved search and what to do with the mail it finds.
//!
//! A rule's query is a search query ([`crate::query`]), so what a rule
//! matches as mail arrives is what the same text finds in search, and the
//! count a preview shows is the count a run changes.

use crate::query::{Cond, MessageFacts, Query, ThreadFacts};

/// What a rule does to a conversation it matches.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    /// Put the label with this id on it.
    Label(String),
    /// Archive it.
    Archive,
    /// Mark it read.
    Read,
    /// Star it.
    Star,
    /// Move it to Junk.
    Junk,
    /// Move it to the Trash.
    Trash,
}

impl Action {
    /// Read the stored spelling: `label:<id>`, `archive`, `read`, `star`,
    /// `junk`, `trash`.
    pub fn parse(s: &str) -> Option<Action> {
        if let Some(id) = s.strip_prefix("label:") {
            return match id.is_empty() {
                true => None,
                false => Some(Action::Label(id.to_string())),
            };
        }
        match s {
            "archive" => Some(Action::Archive),
            "read" => Some(Action::Read),
            "star" => Some(Action::Star),
            "junk" => Some(Action::Junk),
            "trash" => Some(Action::Trash),
            _ => None,
        }
    }

    /// The stored spelling.
    pub fn spelling(&self) -> String {
        match self {
            Action::Label(id) => format!("label:{id}"),
            Action::Archive => "archive".into(),
            Action::Read => "read".into(),
            Action::Star => "star".into(),
            Action::Junk => "junk".into(),
            Action::Trash => "trash".into(),
        }
    }

    /// Whether the action may touch mail the receive pipeline put in Junk.
    /// Archiving it would take it out of Junk; a rule cannot vouch for it.
    pub fn allowed_in_junk(&self) -> bool {
        matches!(
            self,
            Action::Label(_) | Action::Read | Action::Star | Action::Trash
        )
    }
}

/// Why a rule was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The query is empty.
    EmptyQuery,
    /// The query uses `in:`, `is:` or `label:`, which say where a
    /// conversation is or what was done to it — nothing a message has as
    /// it arrives.
    NotForRules,
    /// No actions, or one not in [`Action::parse`].
    BadAction,
}

impl Refused {
    /// The error code the API answers with.
    pub fn code(self) -> &'static str {
        match self {
            Refused::EmptyQuery => "empty_query",
            Refused::NotForRules => "operator_not_for_rules",
            Refused::BadAction => "invalid_action",
        }
    }
}

/// Check a rule before it is saved; the actions in order, duplicates dropped.
pub fn check(query: &str, actions: &[String]) -> Result<Vec<Action>, Refused> {
    let q = Query::parse(query);
    if q.text.is_empty() && !q.has_operators() {
        return Err(Refused::EmptyQuery);
    }
    let arrives_with = q
        .conds
        .iter()
        .all(|(_, c)| !matches!(c, Cond::Is(_) | Cond::Label(_)));
    if q.within.is_some() || !arrives_with {
        return Err(Refused::NotForRules);
    }
    let mut out: Vec<Action> = Vec::new();
    for a in actions {
        let action = Action::parse(a.trim()).ok_or(Refused::BadAction)?;
        if !out.contains(&action) {
            out.push(action);
        }
    }
    if out.is_empty() {
        return Err(Refused::BadAction);
    }
    Ok(out)
}

/// One arriving message, as a rule reads it.
#[derive(Debug, Clone)]
pub struct Arriving<'a> {
    /// Its subject.
    pub subject: &'a str,
    /// Its From header.
    pub from: &'a str,
    /// Its To header.
    pub to: &'a str,
    /// Epoch seconds.
    pub date: i64,
    /// Attachments it carries.
    pub attachments: u32,
    /// Its body text, for the words of a query; empty when there is none.
    pub body: &'a str,
}

/// Whether a rule's query matches one arriving message. Words must each
/// appear, ignoring case, in the subject, the From header or the body;
/// operators read the message alone.
pub fn matches(query: &str, m: &Arriving<'_>) -> bool {
    let q = Query::parse(query);
    let haystack = format!("{}\n{}\n{}", m.subject, m.from, m.body).to_lowercase();
    let words_match = q
        .text
        .split_whitespace()
        .all(|w| haystack.contains(&w.to_lowercase()));
    if !words_match {
        return false;
    }
    let facts = ThreadFacts {
        subject: m.subject,
        senders: m.from,
        latest_date: m.date,
        unread_count: 1,
        starred: false,
        pinned: false,
        labels: &[],
    };
    q.admits(&facts, || {
        vec![MessageFacts {
            sender: m.from.to_string(),
            recipients: m.to.to_string(),
            subject: m.subject.to_string(),
            attachments: Some(m.attachments),
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arriving<'a>(subject: &'a str, from: &'a str, attachments: u32) -> Arriving<'a> {
        Arriving {
            subject,
            from,
            to: "me@golia.jp",
            date: 1_761_955_200,
            attachments,
            body: "Please find the invoice attached.",
        }
    }

    #[test]
    fn actions_round_trip_and_unknown_ones_are_refused() {
        for s in ["label:7", "archive", "read", "star", "junk", "trash"] {
            assert_eq!(Action::parse(s).unwrap().spelling(), s);
        }
        for s in ["label:", "forward", "Archive", ""] {
            assert_eq!(Action::parse(s), None, "{s}");
        }
    }

    #[test]
    fn what_a_rule_cannot_ask() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(check(" ", &a(&["read"])), Err(Refused::EmptyQuery));
        for q in [
            "in:inbox from:x",
            "is:unread",
            "label:work",
            "-is:starred x",
        ] {
            assert_eq!(check(q, &a(&["read"])), Err(Refused::NotForRules), "{q}");
        }
        assert_eq!(check("from:x", &a(&[])), Err(Refused::BadAction));
        assert_eq!(check("from:x", &a(&["forward"])), Err(Refused::BadAction));
        assert_eq!(
            check("from:x", &a(&["read", "label:3", "read"])),
            Ok(vec![Action::Read, Action::Label("3".into())])
        );
    }

    #[test]
    fn a_rule_matches_what_its_search_would() {
        let bill = arriving("Your October invoice", "Billing <noreply@shop.example>", 1);
        assert!(matches("from:shop.example has:attachment", &bill));
        assert!(matches("invoice attached", &bill));
        assert!(matches("subject:october -from:bank", &bill));
        assert!(!matches("from:bank", &bill));
        assert!(!matches("-has:attachment", &bill));
        assert!(!matches("invoice receipt", &bill));
        assert!(matches("to:golia.jp after:2025-10-01", &bill));
    }

    #[test]
    fn junk_keeps_out_what_would_take_it_out_of_junk() {
        assert!(!Action::Archive.allowed_in_junk());
        assert!(!Action::Junk.allowed_in_junk());
        assert!(Action::Trash.allowed_in_junk());
        assert!(Action::Label("1".into()).allowed_in_junk());
    }
}
