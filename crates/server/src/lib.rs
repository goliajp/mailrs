//! The SQL core, `mailrs-pg-core`: fastcore's peer over PostgreSQL or spg.
//! It serves the core-api to `mailrs-webapi`, drains the spool the receiver
//! writes, and runs IMAP, POP3 and ManageSieve.

mod acme;
mod ai_analyzer;
mod body;
mod boot;
mod bootstrap;
mod config;
mod content_worker;
mod core_rpc;
mod domain_store;
mod event_bus;
mod intake;
mod kevy_store;
mod message_store;
mod message_util;
pub(crate) mod permission;
mod pg;
mod pg_schema;
mod protocols;
mod reconcile_task;
mod sieve_store;
mod tls;
mod webhook;

mod ldap_auth {
    pub use mailrs_core::ldap_auth::*;
}
mod kevy_net {
    pub use mailrs_receiver::kevy_net::*;
}
mod kevy_notify {
    pub use mailrs_receiver::kevy_notify::*;
}
mod users {
    pub use mailrs_core::users::*;
}

use bootstrap::*;

use crate::config::ServerConfig;
use crate::users::UserStore;
use mailrs_mailbox::PgMailboxStore;

// Re-export only the event types integration tests need to observe the
// receiving path. The driver itself lives in `test_support`.
pub use event_bus::{BroadcastEvent, EventBus, SmtpEvent};

#[doc(hidden)]
pub mod test_support;

/// Boot the SQL core, fastcore's peer, and block until a shutdown signal.
///
/// Env: everything `ServerConfig::from_env()` reads, which includes
/// `MAILRS_PG_URL` and `MAILRS_MAILDIR`, plus `MAILRS_KEVY_URL` for the shared
/// side-state families, `MAILRS_CORE_RPC_ADDR` and `MAILRS_CORE_API_SECRET`.
pub async fn run_pg_core() {
    boot::run().await;
}
