#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod facts;
mod never;
mod set;

use std::net::IpAddr;

pub use facts::{Facts, Listing, Mark, Marks};
pub use never::NeverList;
pub use set::Set;

/// Where an observation came from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An incident someone wrote up.
    Incident,
    /// The sign-in guard locked this address out.
    LoginGuard,
    /// A feedback-loop complaint.
    Complaint,
    /// Abuse of this server: relaying, posing as its own mail.
    Abuse,
    /// One user moved mail from this sender or link to Junk.
    UserJunk,
    /// One user took mail from this sender out of Junk that the server
    /// had put there.
    UserNotJunk,
    /// An administrator's decision, with its reason.
    Manual,
}

impl Source {
    /// Its name as written in observations: `user_junk`.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Incident => "incident",
            Source::LoginGuard => "login_guard",
            Source::Complaint => "complaint",
            Source::Abuse => "abuse",
            Source::UserJunk => "user_junk",
            Source::UserNotJunk => "user_not_junk",
            Source::Manual => "manual",
        }
    }
}

/// What an observation is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// One address.
    Ip,
    /// A network, `203.0.113.0/24`.
    Cidr,
    /// A domain a message is sent from.
    SenderDomain,
    /// A host a link points at.
    LinkDomain,
}

/// For or against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    /// Against.
    Deny,
    /// For: wins over any deny of the same subject.
    Allow,
    /// Withdraws the observation named in `retracts`.
    Retract,
    /// For, as one reader's word: recorded and counted, and it removes
    /// nothing. Unlike `Allow`, which overrules every deny of its subject,
    /// so one reader keeping one newsletter cannot exempt its domain from
    /// the rules.
    Vouch,
}

/// One fact. Added, never changed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Observation {
    /// Unique id.
    pub id: String,
    /// Unix seconds.
    pub at: i64,
    /// Where it came from.
    pub source: Source,
    /// What it is about.
    pub kind: Kind,
    /// The address, network or domain, as written.
    pub value: String,
    /// For or against.
    pub stance: Stance,
    /// For `Retract`: the id it withdraws.
    #[serde(default)]
    pub retracts: Option<String>,
    /// Why.
    pub reason: String,
    /// The person who recorded it, or who reported it (a `UserJunk` mark's
    /// user); thresholds count distinct ones.
    #[serde(default)]
    pub actor: Option<String>,
}

/// SPF, DKIM and DMARC as the receiver found them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Auth {
    /// DMARC evaluated and failed.
    pub dmarc_fail: bool,
    /// SPF evaluated and failed.
    pub spf_fail: bool,
}

/// The parts of one message the set is asked about; the caller parses the mail.
#[derive(Debug, Clone, Default)]
pub struct Message {
    /// The connecting address.
    pub client_ip: Option<IpAddr>,
    /// The envelope sender's domain.
    pub envelope_domain: String,
    /// The From header's domain.
    pub header_domain: String,
    /// The hosts the body's links point at.
    pub link_hosts: Vec<String>,
    /// Authentication results.
    pub auth: Auth,
}

/// The registrable domain of a host, the private part of the public suffix
/// list included: `a.b.github.io` → `b.github.io`. Lowercase; the host
/// itself when it has no known suffix.
pub fn registrable(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    psl::domain_str(&host).unwrap_or(&host).to_string()
}
