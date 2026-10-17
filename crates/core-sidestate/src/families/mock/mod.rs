//! Disposable test mailboxes in the network kevy.
//!
//! ```text
//! mailrs:mock:ns:{ns}        hash  owner, created_at, smtp_token_hash
//! mailrs:mock:ns:{ns}:dropped int  messages dropped to stay under the cap
//! mailrs:mock:ns-idx         set   every namespace
//! mailrs:mock:owner:{user}   set   the user's namespaces
//! mailrs:mock:ns:{ns}:msgs   zset  message ids, score = arrival ms
//! mailrs:mock:ns:{ns}:tags   hash  tag -> ms it was handed out (field expires with retention)
//! mailrs:mock:msg:{id}       hash  ns, tag, direction, rcpt, size, received_at, summary
//!                                  (expires with the message)
//! mailrs:mock:bytes          int   bytes of every stored message file
//! mailrs:mock:received       int   messages stored, ever
//! mailrs:mock:rejected-unknown-ns  int  SMTP transactions refused for naming no namespace
//! ```
//!
//! Message bodies are files under [`root`], `{ns}/{id}.eml`, so a flooded
//! namespace fills a disk quota instead of the kevy-server's memory.
//!
//! The hash and the zset cannot be written in one transaction, so the
//! zset is the authority: a listed id whose hash is gone has expired and
//! is removed from the zset when it is next read.
//!
//! Every process that touches mock mail (receiver, fastcore, webapi,
//! sender) reads [`domain`]; it is unset, and the feature off, unless
//! `MAILRS_MOCK_DOMAIN` names the domain.

mod messages;
mod namespace;
mod stats;
mod sweep;

pub use messages::*;
pub use namespace::*;
pub use stats::*;
pub use sweep::*;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// How long a message is kept.
pub const RETENTION: Duration = Duration::from_secs(3 * 86_400);
/// Messages kept per namespace; the oldest are dropped past it.
pub const MAX_MESSAGES: usize = 500;
/// Bytes of message files across every namespace; new mail is refused
/// past it.
pub const MAX_TOTAL_BYTES: i64 = 2 * 1024 * 1024 * 1024;
/// Namespaces one user may own.
pub const MAX_NAMESPACES_PER_USER: usize = 10;

static DOMAIN: LazyLock<String> = LazyLock::new(|| {
    std::env::var("MAILRS_MOCK_DOMAIN")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
});

static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    PathBuf::from(std::env::var("MAILRS_MOCK_ROOT").unwrap_or_else(|_| "/data/mock".to_string()))
});

/// The mock domain, or `""` when the feature is off.
pub fn domain() -> &'static str {
    &DOMAIN
}

/// The directory holding every namespace's message files.
pub fn root() -> &'static Path {
    &ROOT
}

/// The mock address `addr` names, under the configured domain.
pub fn parse(addr: &str) -> Option<mailrs_mock::MockAddress> {
    mailrs_mock::parse_address(addr, domain())
}

pub fn is_mock(addr: &str) -> bool {
    mailrs_mock::is_mock_domain(addr, domain())
}

const NS_INDEX: &[u8] = b"mailrs:mock:ns-idx";
const BYTES: &[u8] = b"mailrs:mock:bytes";
const RECEIVED: &[u8] = b"mailrs:mock:received";
const REJECTED_UNKNOWN_NS: &[u8] = b"mailrs:mock:rejected-unknown-ns";

fn ns_key(ns: &str) -> String {
    format!("mailrs:mock:ns:{ns}")
}

fn owner_key(user: &str) -> String {
    format!("mailrs:mock:owner:{}", user.to_ascii_lowercase())
}

fn msgs_key(ns: &str) -> String {
    format!("mailrs:mock:ns:{ns}:msgs")
}

fn dropped_key(ns: &str) -> String {
    format!("mailrs:mock:ns:{ns}:dropped")
}

fn tags_key(ns: &str) -> String {
    format!("mailrs:mock:ns:{ns}:tags")
}

fn msg_key(id: &str) -> String {
    format!("mailrs:mock:msg:{id}")
}

/// Where the message file `id` of namespace `ns` lives under `root`.
pub fn message_path(root: &Path, ns: &str, id: &str) -> PathBuf {
    root.join(ns).join(format!("{id}.eml"))
}

#[derive(Debug)]
pub enum MockError {
    Kevy(kevy_client::KevyError),
    Io(std::io::Error),
    BadName(mailrs_mock::NameError),
    Exists,
    NotFound,
    TooManyNamespaces,
    StoreFull,
}

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kevy(e) => write!(f, "kevy: {e}"),
            Self::Io(e) => write!(f, "io: {e}"),
            Self::BadName(e) => write!(f, "{e}"),
            Self::Exists => f.write_str("this namespace already exists"),
            Self::NotFound => f.write_str("no such namespace"),
            Self::TooManyNamespaces => write!(
                f,
                "a user may own at most {MAX_NAMESPACES_PER_USER} namespaces"
            ),
            Self::StoreFull => f.write_str("the mock store is full"),
        }
    }
}

impl std::error::Error for MockError {}

impl From<kevy_client::KevyError> for MockError {
    fn from(e: kevy_client::KevyError) -> Self {
        Self::Kevy(e)
    }
}

impl From<std::io::Error> for MockError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type MockResult<T> = Result<T, MockError>;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn utf8(v: Vec<u8>) -> String {
    String::from_utf8(v).unwrap_or_default()
}

#[cfg(test)]
mod tests;
