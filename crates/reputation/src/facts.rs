//! What the set says about one message: facts for the rules to judge.

use crate::Source;

/// One observation that lists a subject.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Listing {
    /// What it names: the address, network or domain as recorded.
    pub subject: String,
    /// The observation's id.
    pub id: String,
    /// Its source.
    pub source: Source,
    /// Whole days since it was made.
    pub age_days: i64,
    /// Its reason.
    pub reason: String,
}

/// One user's or reporter's mark on a domain.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Mark {
    /// The observation's id.
    pub id: String,
    /// Its source: `UserJunk` or `Complaint`.
    pub source: Source,
    /// Who made it, when known.
    pub actor: String,
    /// Whole days since it was made.
    pub age_days: i64,
}

/// The marks on one registrable domain, newest first.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Marks {
    /// The registrable domain.
    pub domain: String,
    /// It is on the never-list: no threshold may list it.
    pub never: bool,
    /// Every mark, newest first, at most 500.
    pub marks: Vec<Mark>,
}

/// What the set knows about one message's sender and links.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct Facts {
    /// Listings of the connecting address or a network holding it.
    pub ip: Vec<Listing>,
    /// Listings of the envelope sender's domain or a parent of it.
    pub envelope: Vec<Listing>,
    /// Listings of the From header's domain or a parent of it.
    pub header: Vec<Listing>,
    /// Listings of a link's host or a parent of it.
    pub links: Vec<Listing>,
    /// Marks on the From header's registrable domain.
    pub sender_marks: Option<Marks>,
    /// Marks on each linked registrable domain that has any.
    pub link_marks: Vec<Marks>,
    /// DMARC evaluated and failed.
    pub dmarc_fail: bool,
    /// SPF evaluated and failed.
    pub spf_fail: bool,
}
