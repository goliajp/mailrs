#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

mod table;
pub use table::{ALL, preset_for, preset_for_domain};

/// How a connection is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tls {
    /// TLS from the first byte — IMAPS 993, SMTPS 465.
    Implicit,
    /// Plain, upgraded with `STARTTLS` before anything secret is sent.
    StartTls,
    /// No TLS. Present because some intranet servers still have none;
    /// a set-up screen should make choosing it deliberate.
    None,
}

/// Which protocol talks to this endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// Mail stays on the server and is read in place.
    Imap,
    /// Mail is downloaded, and usually deleted.
    Pop3,
    /// RFC 8620 / 8621.
    Jmap,
    /// Sending.
    Smtp,
}

/// What the person has to supply to be let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// The account password, as typed at the provider's own login.
    Password,
    /// A secret generated in the provider's web UI specifically for
    /// mail clients. **Not** the login password, which is refused with
    /// a message that does not say so.
    AppPassword,
    /// A browser hand-off. The provider will not accept any password.
    OAuth2,
}

/// Where to get the secret this provider wants, for a screen to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretHelp {
    /// What it is called, in the provider's own words — "授权码",
    /// "app password" — because that is the label to look for.
    pub what: &'static str,
    /// The page that generates it.
    pub url: &'static str,
}

/// One server to connect to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Which protocol.
    pub protocol: Protocol,
    /// Hostname.
    ///
    /// `Cow` so the table of known providers can be a `static` and a
    /// guess built at run time can be the same type.
    pub host: Cow<'static, str>,
    /// Port.
    pub port: u16,
    /// How the connection is protected.
    pub tls: Tls,
}

impl Endpoint {
    /// A convenience for the table and for guesses.
    pub fn new(
        protocol: Protocol,
        host: impl Into<Cow<'static, str>>,
        port: u16,
        tls: Tls,
    ) -> Self {
        Self {
            protocol,
            host: host.into(),
            port,
            tls,
        }
    }
}

/// Everything a set-up screen needs about one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preset {
    /// A stable identifier — `gmail`, `outlook`, `qq`.
    pub id: &'static str,
    /// What to call it on screen.
    pub label: &'static str,
    /// The domains that select this preset.
    pub domains: &'static [&'static str],
    /// Where mail is read.
    pub imap: Endpoint,
    /// Where mail is sent.
    pub smtp: Endpoint,
    /// What the person must supply.
    pub auth: AuthKind,
    /// Where to get it, when it is not simply their password.
    pub secret_help: Option<SecretHelp>,
    /// Folders never to sync — a provider's own duplicate views.
    pub skip_folders: &'static [&'static str],
}

/// A step in finding settings for a domain that has no preset.
///
/// Ordered, and the order is the point: a provider's own records are
/// authoritative, the community database is a good guess maintained by
/// somebody, and the conventional hostname is only a guess. Trying the
/// guess first ships settings that appear to work until they do not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Autodiscover {
    /// An RFC 6186 SRV lookup.
    Srv {
        /// The record to query.
        name: String,
        /// What answering it would configure.
        protocol: Protocol,
        /// The protection implied by this record name.
        tls: Tls,
    },
    /// Thunderbird's ISPDB, which most providers are in.
    Ispdb {
        /// The URL to fetch.
        url: String,
    },
    /// The conventional hostnames, tried last.
    Guess {
        /// `imap.<domain>` on 993.
        imap: Endpoint,
        /// `smtp.<domain>` on 587.
        smtp: Endpoint,
    },
}

impl Autodiscover {
    /// The steps to try for a domain, in order.
    pub fn for_domain(domain: &str) -> Vec<Self> {
        let d = domain.trim().trim_matches('.').to_ascii_lowercase();
        vec![
            Self::Srv {
                name: format!("_imaps._tcp.{d}"),
                protocol: Protocol::Imap,
                tls: Tls::Implicit,
            },
            Self::Srv {
                name: format!("_imap._tcp.{d}"),
                protocol: Protocol::Imap,
                tls: Tls::StartTls,
            },
            Self::Srv {
                name: format!("_submissions._tcp.{d}"),
                protocol: Protocol::Smtp,
                tls: Tls::Implicit,
            },
            Self::Srv {
                name: format!("_submission._tcp.{d}"),
                protocol: Protocol::Smtp,
                tls: Tls::StartTls,
            },
            Self::Ispdb {
                url: format!("https://autoconfig.thunderbird.net/v1.1/{d}"),
            },
            Self::Guess {
                imap: Endpoint::new(Protocol::Imap, format!("imap.{d}"), 993, Tls::Implicit),
                smtp: Endpoint::new(Protocol::Smtp, format!("smtp.{d}"), 587, Tls::StartTls),
            },
        ]
    }
}
