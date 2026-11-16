//! The fact ledger's records, the same in every store: what a delivered
//! message was, kept so that a change of reputation, a correction or a
//! report can find the mail it concerns and judge only that again. Facts
//! are written once at delivery and never edited; a verdict is derived
//! from them and the rules, and may be recomputed wholesale.

/// What one delivered message was, as delivery read it.
#[derive(Debug, Clone, Default)]
pub struct LedgerFacts {
    /// The message's Message-ID.
    pub message_id: String,
    /// The reader whose copy this is.
    pub user: String,
    /// The From address, lowercased.
    pub from_addr: String,
    /// The domain of [`Self::from_addr`].
    pub from_domain: String,
    /// The Reply-To address, when the message names one.
    pub reply_to: String,
    /// Every host the message links to.
    pub link_hosts: Vec<String>,
    /// The SHA-256 of each attachment, in hex.
    pub attachment_sha256: Vec<String>,
    /// SPF, DKIM and DMARC results as the receiver recorded them.
    pub auth: String,
    /// Epoch seconds the message arrived.
    pub arrived: i64,
    /// The registrable domain of `from_domain`, which the row is indexed
    /// by, so an observation on a domain finds mail from its subdomains.
    pub from_reg: String,
}

/// A link host to index the message by, with its registrable domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedLink {
    /// The host linked to.
    pub host: String,
    /// Its registrable domain, which the row is found by.
    pub reg: String,
}

/// What the rules made of a message: derived, recomputed at will.
#[derive(Debug, Clone, Default)]
pub struct LedgerVerdict {
    /// The rules that matched.
    pub rules: Vec<String>,
    /// The score the rules gave.
    pub score: f64,
    /// The version of the rules that judged it.
    pub rules_version: String,
    /// Epoch seconds the verdict was given.
    pub judged_at: i64,
    /// The fraud rules hold the conversation.
    pub holds: bool,
    /// What the screening rules matched, in whichever mode they run.
    pub screening: Vec<String>,
}
