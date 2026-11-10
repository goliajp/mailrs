//! A domain registered to send cold outreach, not to be visited.
//!
//! `ejohnson@dealclosersvoice.com`, `natalierober…@247dealclosers.com`:
//! both web roots answer with a redirect to `dealclosers.co`. Outreach
//! firms buy look-alike domains by the dozen, put a mailbox on each and
//! point the website at the brand they sell, so the brand's own domain
//! never carries the complaints.
//!
//! Two facts the message and its sender's own infrastructure prove, with
//! no reading of the words:
//!
//! - [`hosted_mailbox`]: it was written in a person's Microsoft 365 or
//!   Google mailbox, not sent by a mailing service. Notices from big
//!   companies whose web roots also redirect (Synology, Atlassian, AWS,
//!   Shopify) all leave through SES, Mailgun, SparkPost or their own
//!   relays.
//! - [`lands_elsewhere`]: `http://<from domain>/` ends on a different
//!   registrable domain.
//!
//! # Suspicion, never a verdict
//!
//! Measured over a year of production mail (29,507 messages, 1,104 sender
//! domains): with both facts, no unsubscribe header and a first contact,
//! 25 sender domains match and every one is cold outreach. A real company
//! can still write one-to-one from a secondary domain, so this scores and
//! never hides: the message goes to Junk, where the reader sees it.

/// Whether the message was written in a hosted personal mailbox: a
/// Microsoft 365 tenant or Google (Gmail or Workspace).
///
/// Read from what those services stamp and a sender does not choose:
/// Exchange Online's internal hops (`*.prod.outlook.com`,
/// `*.outbound.protection.outlook.com`) in the `Received` chain, and
/// Gmail's `Message-ID` host.
pub fn hosted_mailbox(raw: &[u8]) -> bool {
    let head = header_block(raw);
    let mut hosted = false;
    for field in unfolded_fields(head) {
        let lower = field.to_ascii_lowercase();
        if lower.starts_with("received:")
            && (lower.contains(".prod.outlook.com")
                || lower.contains(".outbound.protection.outlook.com"))
        {
            hosted = true;
        }
        if lower.starts_with("message-id:") && lower.trim_end().ends_with("@mail.gmail.com>") {
            hosted = true;
        }
    }
    hosted
}

/// Mail providers whose own web roots redirect to a sign-in page:
/// `gmail.com` lands on `accounts.google.com`. A sender at one of them
/// owns a mailbox, not the domain, so where its web root goes says
/// nothing about them. A list of exemptions, which only ever lets mail
/// through; the default of [`crate::Policy::mailbox_providers`],
/// replaced by `MAILRS_FRAUD_MAILBOX_PROVIDERS`.
pub const DEFAULT_MAILBOX_PROVIDERS: &[&str] = &[
    "google.com",
    "live.com",
    "outlook.com",
    "microsoft.com",
    "yahoo.com",
    "yahoo.co.jp",
    "apple.com",
    "icloud.com",
    "aol.com",
    "proton.me",
];

/// Public suffixes whose registry admits only a qualified registrant,
/// one domain per organisation: JPRS's attribute types (a company holds
/// one `co.jp`, a school one `ac.jp`) and the US `edu` and `gov`. A firm
/// cannot buy dozens of them to burn, so a sender under one is a real
/// organisation writing from its own domain. Suffixes anyone can
/// register (`jp`, `com`) are not here. The default of
/// [`crate::Policy::vetted_suffixes`], replaced by
/// `MAILRS_FRAUD_VETTED_SUFFIXES`.
pub const DEFAULT_VETTED_SUFFIXES: &[&str] = &[
    "co.jp", "or.jp", "ne.jp", "ac.jp", "ad.jp", "go.jp", "lg.jp", "ed.jp", "gr.jp", "edu", "gov",
];

/// The mailbox providers from `MAILRS_FRAUD_MAILBOX_PROVIDERS` (comma
/// separated), or [`DEFAULT_MAILBOX_PROVIDERS`] when it is not set.
pub fn mailbox_providers_from_env() -> Vec<String> {
    list_from_env("MAILRS_FRAUD_MAILBOX_PROVIDERS", DEFAULT_MAILBOX_PROVIDERS)
}

/// The vetted suffixes from `MAILRS_FRAUD_VETTED_SUFFIXES` (comma
/// separated), or [`DEFAULT_VETTED_SUFFIXES`] when it is not set.
pub fn vetted_suffixes_from_env() -> Vec<String> {
    list_from_env("MAILRS_FRAUD_VETTED_SUFFIXES", DEFAULT_VETTED_SUFFIXES)
}

fn list_from_env(name: &str, default: &[&str]) -> Vec<String> {
    match std::env::var(name) {
        Ok(v) => v
            .split(',')
            .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => default.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// Whether `host` sits under one of `suffixes`: `example.co.jp` under
/// `co.jp`, `cs.mit.edu` under `edu`. The suffix itself is no sender.
pub fn under_vetted_suffix(host: &str, suffixes: &[String]) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    suffixes
        .iter()
        .any(|s| host.len() > s.len() + 1 && host.ends_with(&format!(".{s}")))
}

/// Whether a visit to the sender's web root that ended on `landing_host`
/// left the sender's registrable domain for one that is not a mailbox
/// provider's sign-in page (`providers`, registrable domains).
pub fn lands_elsewhere(from_host: &str, landing_host: &str, providers: &[String]) -> bool {
    let from = crate::brand::registrable(from_host);
    let landed = crate::brand::registrable(landing_host);
    if from.is_empty() || landed.is_empty() || from == landed {
        return false;
    }
    !providers.contains(&landed)
}

/// Whether a message is worth the visit to its sender's web root: one
/// person writing to the reader for the first time from a hosted mailbox,
/// with nothing marking it as list mail, to someone who never wrote to
/// that domain. Anything else is answered without leaving the process,
/// so the visits stay rare.
pub fn worth_visiting(hosted: bool, is_bulk: bool, relation: &crate::relation::Relation) -> bool {
    hosted && !is_bulk && relation.first_contact && !relation.from_correspondent_domain
}

/// Whether `domain`, a domain that authenticated the message (the SPF
/// envelope domain or a DKIM signature's `d=`), speaks for `from_host`:
/// the same registrable domain.
///
/// The visit is paid for only when the `From` domain itself passed SPF or
/// DKIM. The two facts that make a message worth visiting, a hosted
/// mailbox's `Received` hop and a Gmail `Message-ID`, are text a sender
/// can write; without this anyone could make the receiver fetch any
/// public host by sending forged mail with a fresh `From` domain each
/// time. Cold outreach authenticates as its own domain, so the rule
/// misses nothing by asking.
pub fn speaks_for(from_host: &str, domain: &str) -> bool {
    let from = crate::brand::registrable(from_host);
    !from.is_empty() && from == crate::brand::registrable(domain)
}

/// What the cache holds for a sender host: where its web root landed, or
/// that it did not answer. A host that did not answer is not evidence of
/// anything, so it is kept for a shorter time and read as not elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing {
    /// The final host after redirects.
    At(String),
    /// No answer: refused, timed out, not an HTTP site.
    Nowhere,
}

impl Landing {
    /// The cache value: `at:<host>` or `nowhere`.
    pub fn encode(&self) -> String {
        match self {
            Landing::At(host) => format!("at:{host}"),
            Landing::Nowhere => "nowhere".to_string(),
        }
    }

    /// The cache value read back; `None` for anything else.
    pub fn decode(value: &str) -> Option<Self> {
        if value == "nowhere" {
            return Some(Landing::Nowhere);
        }
        value
            .strip_prefix("at:")
            .map(|host| Landing::At(host.to_string()))
    }

    /// Whether this landing took the visitor off `from_host`'s domain to
    /// one that is not among `providers`.
    pub fn elsewhere(&self, from_host: &str, providers: &[String]) -> bool {
        match self {
            Landing::At(host) => lands_elsewhere(from_host, host, providers),
            Landing::Nowhere => false,
        }
    }

    /// The registrable domain it landed on; empty when it did not answer.
    pub fn registrable(&self) -> String {
        match self {
            Landing::At(host) => crate::brand::registrable(host),
            Landing::Nowhere => String::new(),
        }
    }
}

/// How long a landing is kept: a month for an answer, a day for none.
pub fn ttl_seconds(landing: &Landing) -> u64 {
    match landing {
        Landing::At(_) => 30 * 24 * 3600,
        Landing::Nowhere => 24 * 3600,
    }
}

fn header_block(raw: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        if rest.starts_with(b"\r\n\r\n") || rest.starts_with(b"\n\n") {
            return &raw[..i];
        }
        i += 1;
    }
    raw
}

fn unfolded_fields(head: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(head);
    let mut fields: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = fields.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
            continue;
        }
        fields.push(line.to_string());
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    const M365: &[u8] = b"Received: from MA0PR01CU012.outbound.protection.outlook.com (40.107.57.125:56982)\r\n\tby mail.golia.ai with ESMTP\r\nFrom: Emily Johnson <ejohnson@dealclosersvoice.com>\r\nMessage-ID: <4ac94ee1@dealclosersvoice.com>\r\n\r\nHao,\r\n";
    const GMAIL: &[u8] = b"Received: from mail-oo1-f47.google.com (mail-oo1-f47.google.com.)\n\tby mail.golia.ai\nFrom: brooke@hrolooksup.com\nMessage-ID:\n <CAJx=abc@mail.gmail.com>\n\nHi\n";
    const SES: &[u8] = b"Received: from a9-110.smtp-out.amazonses.com\r\nFrom: no-reply@amazonaws.com\r\nMessage-ID: <0100019815bb2d01@email.amazonses.com>\r\nList-Unsubscribe: <mailto:x>\r\n\r\nbody mentions mail.gmail.com> and .prod.outlook.com\r\n";

    #[test]
    fn exchange_online_hops_mark_a_hosted_mailbox() {
        assert!(hosted_mailbox(M365));
    }

    #[test]
    fn a_folded_gmail_message_id_marks_a_hosted_mailbox() {
        assert!(hosted_mailbox(GMAIL));
    }

    #[test]
    fn a_mailing_service_is_not_a_hosted_mailbox_whatever_the_body_says() {
        assert!(!hosted_mailbox(SES));
    }

    fn providers() -> Vec<String> {
        DEFAULT_MAILBOX_PROVIDERS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_burner_domain_lands_on_its_brand() {
        let p = providers();
        assert!(lands_elsewhere(
            "dealclosersvoice.com",
            "dealclosers.co",
            &p
        ));
        assert!(lands_elsewhere(
            "smartersupplysync.com",
            "group-smartersupply.com",
            &p
        ));
    }

    #[test]
    fn staying_on_the_domain_is_not_elsewhere() {
        let p = providers();
        assert!(!lands_elsewhere("example.com", "www.example.com", &p));
        assert!(!lands_elsewhere("mail.example.co.jp", "example.co.jp", &p));
    }

    #[test]
    fn a_mailbox_provider_sign_in_page_is_not_elsewhere() {
        let p = providers();
        assert!(!lands_elsewhere("gmail.com", "accounts.google.com", &p));
        assert!(!lands_elsewhere("hotmail.com", "outlook.live.com", &p));
        // and the list is the policy's: without it, the sign-in page is elsewhere
        assert!(lands_elsewhere("gmail.com", "accounts.google.com", &[]));
    }

    #[test]
    fn a_sender_under_a_vetted_suffix_is_recognised_and_the_suffix_alone_is_not() {
        let v: Vec<String> = DEFAULT_VETTED_SUFFIXES
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(under_vetted_suffix("example-group.co.jp", &v));
        assert!(under_vetted_suffix("mail.example.ac.jp", &v));
        assert!(under_vetted_suffix("cs.mit.edu", &v));
        assert!(!under_vetted_suffix("co.jp", &v));
        assert!(!under_vetted_suffix("example.jp", &v));
        assert!(!under_vetted_suffix("dealclosersvoice.com", &v));
        // `xco.jp` is a .jp domain, not a co.jp one
        assert!(!under_vetted_suffix("xco.jp", &v));
    }

    #[test]
    fn landings_round_trip_through_the_cache_value() {
        for landing in [Landing::At("dealclosers.co".into()), Landing::Nowhere] {
            assert_eq!(Landing::decode(&landing.encode()), Some(landing));
        }
        assert_eq!(Landing::decode("garbage"), None);
        let p = providers();
        assert!(!Landing::Nowhere.elsewhere("dealclosersvoice.com", &p));
        assert!(Landing::At("dealclosers.co".into()).elsewhere("dealclosersvoice.com", &p));
        assert_eq!(
            Landing::At("www.dealclosers.co".into()).registrable(),
            "dealclosers.co"
        );
    }

    #[test]
    fn only_the_from_domain_itself_speaks_for_it() {
        assert!(speaks_for("dealclosersvoice.com", "dealclosersvoice.com"));
        assert!(speaks_for("mail.example.co.jp", "example.co.jp"));
        // a mailbox service signing for the tenant is not the tenant
        assert!(!speaks_for("dealclosersvoice.com", "onmicrosoft.com"));
        assert!(!speaks_for("dealclosersvoice.com", "dealclosers.co"));
        assert!(!speaks_for("", ""));
    }

    #[test]
    fn only_a_first_one_to_one_contact_from_a_hosted_mailbox_is_visited() {
        let first = crate::relation::Relation {
            first_contact: true,
            ..Default::default()
        };
        assert!(worth_visiting(true, false, &first));
        assert!(!worth_visiting(false, false, &first));
        assert!(!worth_visiting(true, true, &first));
        let known = crate::relation::Relation {
            first_contact: false,
            ..Default::default()
        };
        assert!(!worth_visiting(true, false, &known));
        let wrote_to_them = crate::relation::Relation {
            first_contact: true,
            from_correspondent_domain: true,
            ..Default::default()
        };
        assert!(!worth_visiting(true, false, &wrote_to_them));
    }
}
