//! What the admin pages change in the receiver without a restart: the
//! domains it accepts mail for, and the largest message it takes.
//!
//! Everything else the receiver reads is fixed at boot from the
//! environment; these two are read again while it runs.

mod local_domains;
mod size_limit;

pub use local_domains::{
    LocalDomains, refresh_and_log, refresh_local_domains, spawn_local_domains_refresh,
};
pub use size_limit::{CONFIG_KEY, MAX_MESSAGE_SIZE_FIELD, MessageSizeLimit};

pub struct RuntimeSettings {
    pub local_domains: LocalDomains,
    pub max_message_size: MessageSizeLimit,
}

/// `MAILRS_LOCAL_DOMAINS` only and the compile-time size limit, never
/// refreshed: the monolith and the tests.
impl From<Vec<String>> for RuntimeSettings {
    fn from(env_domains: Vec<String>) -> Self {
        Self {
            local_domains: LocalDomains::new(env_domains),
            max_message_size: MessageSizeLimit::new(None),
        }
    }
}
