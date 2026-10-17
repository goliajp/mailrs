//! What the admin pages change in the receiver without a restart: the
//! domains it accepts mail for, the largest message it takes, and the
//! sender lists.
//!
//! Everything else the receiver reads is fixed at boot from the
//! environment; these are read again while it runs.

mod local_domains;
mod size_limit;

pub use local_domains::{
    LocalDomains, refresh_and_log, refresh_local_domains, spawn_local_domains_refresh,
};
pub use size_limit::{CONFIG_KEY, MAX_MESSAGE_SIZE_FIELD, MessageSizeLimit};

pub struct RuntimeSettings {
    pub local_domains: LocalDomains,
    pub max_message_size: MessageSizeLimit,
    /// The admin's black and white lists, also read by the pipeline.
    pub sender_lists: crate::greylist_local::GreylistLocalHandle,
}

/// `MAILRS_LOCAL_DOMAINS` only, the compile-time size limit and no sender
/// lists, never refreshed: the monolith and the tests.
impl From<Vec<String>> for RuntimeSettings {
    fn from(env_domains: Vec<String>) -> Self {
        Self {
            local_domains: LocalDomains::new(env_domains),
            max_message_size: MessageSizeLimit::new(None),
            sender_lists: crate::greylist_local::empty(),
        }
    }
}
