//! What every JMAP method runs with: the caller and the core.

use std::sync::Arc;

use mailrs_core_sidestate::families::audit::Credential;
use mailrs_jmap::error::JmapMethodError;

use crate::WebState;
use crate::session::confirm::{Refusal, SessionRef};

pub struct Ctx {
    pub state: Arc<WebState>,
    /// The signed-in account; the only `accountId` any call may name.
    pub user: String,
    /// The agent key the request came with, when it came with one.
    pub key: Option<crate::session::AgentKey>,
    /// The session the request came with, when it came with one. Neither
    /// this nor `key` is a password over HTTP Basic.
    pub session: Option<SessionRef>,
}

/// Why a conversation cannot be deleted for good by this request.
pub enum Forbidden {
    AgentKey,
    ConfirmRequired,
    Unavailable,
}

impl Ctx {
    pub fn new(state: Arc<WebState>, user: String, key: Option<crate::session::AgentKey>) -> Self {
        Self {
            state,
            user,
            key,
            session: None,
        }
    }

    pub fn with_session(mut self, session: Option<SessionRef>) -> Self {
        self.session = session;
        self
    }

    /// Whether this request may delete mail for good, and as what: never
    /// with an agent key; with a session only after a recent password
    /// confirmation; with a password over Basic, as IMAP may.
    pub fn may_delete_for_good(&self) -> Result<Credential, Forbidden> {
        if self.key.is_some() {
            return Err(Forbidden::AgentKey);
        }
        let Some(session) = &self.session else {
            return Ok(Credential::Password);
        };
        match crate::session::confirm::require(Some(session)) {
            Ok(_) => Ok(Credential::Session),
            Err(Refusal::ConfirmRequired | Refusal::AgentKeyNotAllowed) => {
                Err(Forbidden::ConfirmRequired)
            }
            Err(Refusal::Unavailable(_)) => Err(Forbidden::Unavailable),
        }
    }

    /// `accountId` must be the caller's own (RFC 8620 §3.6.2
    /// `accountNotFound` otherwise); absent means the primary account.
    pub fn check_account(&self, args: &serde_json::Value) -> Result<(), JmapMethodError> {
        match args.get("accountId").and_then(|v| v.as_str()) {
            None => Ok(()),
            Some(a) if a.eq_ignore_ascii_case(&self.user) => Ok(()),
            Some(_) => Err(JmapMethodError::AccountNotFound),
        }
    }
}

/// A core failure as a JMAP `serverFail`.
pub fn fail(e: impl std::fmt::Display) -> JmapMethodError {
    JmapMethodError::ServerFail(e.to_string())
}

/// The ids a `/get` asks for: `None` is every object (`ids: null`).
pub fn requested_ids(args: &serde_json::Value) -> Option<Vec<String>> {
    args.get("ids")?.as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    })
}
