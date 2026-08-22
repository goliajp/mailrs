//! Mailboxes somewhere else, on kevy.
//!
//! ```text
//!   ext:accts:{user}          hash    id → JSON AccountRow
//!   ext:secret:{user}:{id}    string  sealed by mailrs-secretbox
//!   ext:sync:{id}:{folder}    hash    uidvalidity, highest_uid, at
//! ```
//!
//! **The secret is not on the row.** A row is listed, logged, and sent
//! to three clients; a token that rides along in it leaks by every one
//! of those paths. It lives under its own key, sealed, and the only
//! code that opens it is the worker about to connect.
//!
//! `last_error` and `state` are stored fields rather than log lines,
//! for the reason `calendar_feeds` gives: the person added this account
//! and is the only one who can fix a rotated password. An account that
//! has been failing since Tuesday has to say so where it was added —
//! silence there means somebody believes they are seeing all their mail
//! when they are not.

use serde::{Deserialize, Serialize};

/// How a connection is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tls {
    /// TLS from the first byte.
    #[default]
    Implicit,
    /// Plain, upgraded before anything secret is sent.
    StartTls,
    /// None. Some intranet servers still have none.
    None,
}

/// What the person supplies to be let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// Their account password.
    #[default]
    Password,
    /// A secret generated for mail clients — QQ's 授权码, Apple's
    /// app-specific password. Not the login password.
    AppPassword,
    /// A browser hand-off; no password is accepted at all.
    OAuth2,
}

/// Whether this account is working.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Syncing.
    #[default]
    Ok,
    /// The credential was rejected. **Waiting will not fix this** —
    /// somebody has to re-authenticate, so it is not retried on a
    /// timer and the row asks for attention instead.
    NeedsAuth,
    /// Something else went wrong; retried with backoff.
    Error,
    /// Deliberately switched off by its owner.
    Paused,
}

/// One server to connect to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Endpoint {
    /// `imap`, `pop3`, `jmap` or `smtp`.
    pub protocol: String,
    /// Hostname.
    pub host: String,
    /// Port.
    pub port: u16,
    /// How the connection is protected.
    #[serde(default)]
    pub tls: Tls,
}

/// An account somewhere else, as stored.
///
/// Every field added after the first release carries `#[serde(default)]`
/// so a row written before it loads rather than failing the whole list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AccountRow {
    /// Opaque, unique within the user.
    pub id: String,
    /// The address at the provider.
    pub email: String,
    /// What the owner called it — "Work", "大学".
    pub display_name: String,
    /// A `mailrs-mailprovider` preset id, or `custom`.
    pub provider: String,
    /// Where mail is read.
    pub incoming: Endpoint,
    /// Where mail is sent.
    pub outgoing: Endpoint,
    /// What the person supplies.
    pub auth: AuthKind,
    /// Login name, when it is not the address.
    #[serde(default)]
    pub username: Option<String>,
    /// The dot beside every row from this account.
    #[serde(default)]
    pub colour: Option<String>,
    /// Whether it is working.
    #[serde(default)]
    pub state: State,
    /// Why the last attempt failed. Cleared by a success.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Consecutive failures, for the backoff.
    #[serde(default)]
    pub failures: u32,
    /// When a sync last succeeded. Zero means never.
    #[serde(default)]
    pub last_sync: i64,
    /// When the next attempt is allowed.
    #[serde(default)]
    pub next_attempt: i64,
    /// When the account was added.
    #[serde(default)]
    pub created_at: i64,
    /// Where it sits in the account list.
    #[serde(default)]
    pub sort: i64,
}

/// Why an account cannot work, in words its owner can act on.
pub fn validate(row: &AccountRow) -> Result<(), String> {
    if !looks_like_an_address(&row.email) {
        return Err(format!("{} is not an email address", row.email));
    }
    check_endpoint(&row.incoming, &["imap", "pop3", "jmap"], "reading")?;
    check_endpoint(&row.outgoing, &["smtp"], "sending")?;
    Ok(())
}

fn check_endpoint(e: &Endpoint, allowed: &[&str], doing: &str) -> Result<(), String> {
    if e.host.trim().is_empty() {
        return Err(format!("no host for {doing}"));
    }
    if e.port == 0 {
        return Err(format!("port 0 cannot be used for {doing}"));
    }
    let p = e.protocol.to_ascii_lowercase();
    if !allowed.contains(&p.as_str()) {
        return Err(format!(
            "{p} is not a protocol for {doing} — expected {}",
            allowed.join(" or ")
        ));
    }
    Ok(())
}

fn looks_like_an_address(v: &str) -> bool {
    match v.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !v.contains(char::is_whitespace)
        }
        None => false,
    }
}

/// The palette a row's dot comes from.
///
/// Chosen to stay apart on both themes, and picked by hashing the id so
/// nobody has to choose and so the colour does not move when accounts
/// are reordered — a dot that changes place teaches nothing.
pub const PALETTE: [&str; 8] = [
    "#3b82f6", "#22c55e", "#f59e0b", "#a855f7", "#ec4899", "#14b8a6", "#ef4444", "#6366f1",
];

/// This account's colour.
pub fn colour_for(id: &str) -> &'static str {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    PALETTE[(h % PALETTE.len() as u64) as usize]
}

/// Shortest gap between syncs of one account.
pub const MIN_SYNC_SECS: i64 = 60;

/// Longest the backoff may reach.
///
/// Bounded because the fix is usually at the other end and nobody tells
/// us when it lands: an account that has been failing for a month must
/// still be tried today.
pub const MAX_BACKOFF_SECS: i64 = 6 * 3600;

/// How long to wait after `failures` consecutive failures.
pub fn next_backoff(failures: u32) -> i64 {
    if failures == 0 {
        return MIN_SYNC_SECS;
    }
    let shift = failures.min(20);
    MIN_SYNC_SECS
        .saturating_mul(1i64.checked_shl(shift).unwrap_or(i64::MAX))
        .min(MAX_BACKOFF_SECS)
}

/// Whether this account should be synced now.
///
/// A never-synced account is due at once, so adding one produces mail
/// without waiting out an interval. A rejected credential is never due:
/// retrying it cannot succeed, and some providers count the attempts.
pub fn is_due(row: &AccountRow, now: i64) -> bool {
    match row.state {
        State::NeedsAuth | State::Paused => false,
        _ if row.last_sync == 0 => true,
        _ => now >= row.next_attempt,
    }
}

/// The row after a sync that worked.
pub fn with_success(mut row: AccountRow, now: i64) -> AccountRow {
    row.last_sync = now;
    row.failures = 0;
    row.state = State::Ok;
    // Cleared, so a recovered account does not keep showing last week's
    // failure beside a fresh timestamp.
    row.last_error = None;
    row.next_attempt = now + MIN_SYNC_SECS;
    row
}

/// The row after a sync that did not.
///
/// A rejected credential is told apart from everything else here,
/// because the two need different words on screen and different
/// behaviour from the worker.
pub fn with_failure(mut row: AccountRow, now: i64, why: &str) -> AccountRow {
    row.failures = row.failures.saturating_add(1);
    row.last_error = Some(why.to_string());
    row.state = if is_a_rejected_credential(why) {
        State::NeedsAuth
    } else {
        State::Error
    };
    row.next_attempt = now + next_backoff(row.failures);
    row
}

/// Whether a failure means the credential is no longer accepted.
///
/// Matched on what the servers actually say: IMAP's `AUTHENTICATIONFAILED`
/// response code (RFC 5530), SMTP's 535, and the words the big providers
/// put in the text.
fn is_a_rejected_credential(why: &str) -> bool {
    let w = why.to_ascii_uppercase();
    w.contains("AUTHENTICATIONFAILED")
        || w.contains("AUTHORIZATIONFAILED")
        || w.contains("INVALID CREDENTIALS")
        || w.contains("INVALID_GRANT")
        || w.contains("535")
        || w.contains("LOGIN FAILED")
}
