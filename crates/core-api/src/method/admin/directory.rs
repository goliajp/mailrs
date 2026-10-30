//! Accounts, aliases, domains — the directory.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

pub const PATH_LIST_TEMPLATES: &str = "/v1/users/{user}/templates";

pub const PATH_DELETE_TEMPLATE: &str = "/v1/users/{user}/templates/{id}";

pub const PATH_CREATE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions";

pub const PATH_LIST_WEBHOOKS: &str = "/v1/admin/accounts/{address}/webhook-subscriptions";

pub const PATH_DELETE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions/{id}";

// ════════════════════════════════════════════════════════════════════
// Wire types — accounts
// ════════════════════════════════════════════════════════════════════

/// One row in the accounts table — public shape (no password hash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountWire {
    pub address: UserAddress,
    pub domain: String,
    pub display_name: String,
    pub active: bool,
    /// Epoch seconds.
    pub created_at: i64,
    pub quota_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_email: Option<String>,
    /// When an administrator cleared the password (epoch seconds), until a
    /// new one is set. An account without a password that was never
    /// cleared has none: the two mean different things to its owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_cleared_at: Option<i64>,
    /// Who cleared it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_cleared_by: Option<String>,
}

/// Internal lookup response — includes Argon2 hash. Only the SMTP/IMAP/POP3/
/// MgSieve AUTH path reads this; never exposed to webapi public API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountWithHashWire {
    #[serde(flatten)]
    pub public: AccountWire,
    /// Argon2 password hash (sensitive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAccountRequest {
    pub address: UserAddress,
    pub display_name: String,
    /// Plaintext password — server hashes with Argon2 before insert.
    /// Empty when `password_hash` is given.
    #[serde(default)]
    pub password: String,
    /// A stored hash to keep as it is, in place of `password`: an argon2
    /// one, or empty for an account without a password. For copying an
    /// account between cores; webapi never passes one on, so an
    /// administrator cannot set someone's password to a value they know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

/// What a new account's password starts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialPassword<'a> {
    /// Hash this plaintext.
    Plain(&'a str),
    /// Store this hash: argon2, or empty for no password.
    Hash(&'a str),
}

impl AddAccountRequest {
    /// The password to start with, or `None` when the request gives
    /// neither a password nor an acceptable hash: a hash that is not
    /// argon2 would be a value no sign-in can match, or one compared as
    /// text by a store that still did.
    pub fn initial_password(&self) -> Option<InitialPassword<'_>> {
        match self.password_hash.as_deref() {
            Some(h) if h.is_empty() || h.starts_with("$argon2") => Some(InitialPassword::Hash(h)),
            Some(_) => None,
            None if self.password.is_empty() => None,
            None => Some(InitialPassword::Plain(&self.password)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccountRequest {
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GetQuotaResponse {
    pub quota_bytes: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SetQuotaRequest {
    pub quota_bytes: i64,
}

/// Body of `GET` and `PUT /v1/admin/accounts/{address}/send-as`: the
/// addresses an administrator has granted this account to send as. The
/// account's own address, the aliases that point at it and the email
/// groups it belongs to are allowed without being listed here;
/// [`EffectivePermissionsResponse::send_as`](super::EffectivePermissionsResponse)
/// is the union of all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendAsList {
    pub send_as: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecoveryEmailRequest {
    pub recovery_email: String,
}

/// Request body for `POST /v1/admin/accounts/{address}/password`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPasswordRequest {
    /// Argon2-hashed password. Webapi hashes the plaintext locally so
    /// fastcore only ever sees the hash on the wire.
    pub password_hash: String,
}

/// Response of `POST /v1/admin/accounts/{address}/password`. The core sets
/// the hash and nothing else: whoever calls it directly (the host's
/// maintenance use) signs no session out and revokes no agent key, and
/// the answer says so. Webapi's routes do both themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetPasswordResponse {
    pub sessions_revoked: bool,
    pub agent_keys_revoked: bool,
}

impl SetPasswordResponse {
    pub const NOTHING_REVOKED: Self = Self {
        sessions_revoked: false,
        agent_keys_revoked: false,
    };
}

/// Request body for `POST /v1/admin/accounts/{address}/password:clear`:
/// empty the hash, so no password signs in, and record who did it. Setting
/// a password afterwards removes the record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearPasswordRequest {
    pub by: String,
}

/// Request body for `POST /v1/users/{user}/messages/{uid}/flags`.
///
/// Fastcore reads the current wire from the per-user uid index,
/// rewrites `flags` verbatim, and (if `\Seen` toggled) mirrors the
/// change to the thread's `has_unread` zset via `mark_seen` /
/// `mark_unread`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SetMessageFlagsRequest {
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountListResponse {
    pub items: Vec<AccountWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — aliases
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasWire {
    pub id: i64,
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
    /// One of: `alias` / `forward` / etc.
    pub alias_type: String,
    pub active: bool,
    pub created_at: i64,
}

/// `GET` — whether mail to `address` would be delivered here, answered
/// without changing anything. `500` when a store it consults cannot be
/// read: that is not the same as "no such recipient".
pub const PATH_RECIPIENT_DELIVERABLE: &str = "/v1/admin/recipients/{address}/deliverable";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RecipientDeliverableResponse {
    pub deliverable: bool,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — domains
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainWire {
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDomainRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainListResponse {
    pub items: Vec<DomainWire>,
}

#[cfg(test)]
mod initial_password_tests {
    use super::*;

    fn req(password: &str, hash: Option<&str>) -> AddAccountRequest {
        AddAccountRequest {
            address: "a@example.test".into(),
            display_name: String::new(),
            password: password.into(),
            password_hash: hash.map(str::to_string),
        }
    }

    #[test]
    fn a_hash_is_kept_only_when_it_is_argon2_or_empty() {
        assert_eq!(
            req("", Some("$argon2id$x")).initial_password(),
            Some(InitialPassword::Hash("$argon2id$x"))
        );
        assert_eq!(
            req("", Some("")).initial_password(),
            Some(InitialPassword::Hash(""))
        );
        assert_eq!(req("", Some("clear-text-1")).initial_password(), None);
        // a hash given wins over a password, and a bad one is not rescued by it
        assert_eq!(req("pw", Some("clear-text-1")).initial_password(), None);
    }

    #[test]
    fn without_a_hash_the_password_is_hashed_and_must_be_there() {
        assert_eq!(
            req("pw", None).initial_password(),
            Some(InitialPassword::Plain("pw"))
        );
        assert_eq!(req("", None).initial_password(), None);
    }
}
