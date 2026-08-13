mod account_store;
mod acme;
mod ai_analyzer;
mod api_key_store;
mod config;
mod conn_metrics;
mod metrics;

mod content_worker;
mod conversation_cache;

mod calendar;
mod dmarc_report;
mod domain_store;
mod event_bus;
mod health;
pub(crate) mod permission;

/// re-export shim: `LdapConfig` moved to the shared `mailrs-core` crate
/// (S5.2h). Kept as `crate::ldap_auth` so the smtp / imap / pop3 /
/// managesieve / web / config call sites stay unchanged.
mod ldap_auth {
    pub use mailrs_core::ldap_auth::*;
}

mod imap_session;
pub mod inbound;
mod inline_image;
/// re-export shim: the generic TCP listener template moved to
/// mailrs-receiver (P6-S5) so the receiver binary can bind its SMTP
/// listeners. server's imap/pop3/web/managesieve listeners use it via this.
mod listeners {
    pub use mailrs_receiver::listeners::*;
}
mod managesieve_session;
mod message_store;
mod message_util;
mod outbound_tls_rpt;
mod pg;
#[cfg(feature = "core-rpc")]
mod pg_core_boot;
mod pop3_session;
mod quota_store;
mod rbl_monitor;
mod reconcile_task;
mod reputation;

mod bootstrap;
mod greylist_backfill;
// greylist_local keeps the spg-bound PG loaders + re-exports the pure
// snapshot/matching half from mailrs-receiver (S5.3).
mod greylist_local;
/// re-export shim: the remote-whitelist sync (spg-free) moved to
/// mailrs-receiver (S5.3).
mod greylist_sync {
    pub use mailrs_receiver::greylist_sync::*;
}
/// re-export shim: the network kevy client moved to mailrs-receiver (P6-S5)
/// so the receiver binary can construct the network anti backends.
pub mod kevy_net {
    pub use mailrs_receiver::kevy_net::*;
}
/// re-export shim: cross-process notify (publisher + subscriber bridge)
/// moved to mailrs-receiver (P6-S5) — the receiver publishes SpoolDelivered,
/// the core spawns the subscriber bridge. Both via this shim.
pub mod kevy_notify {
    pub use mailrs_receiver::kevy_notify::*;
}
#[cfg(feature = "core-rpc")]
mod core_rpc;
mod kevy_store;
mod mcp;
mod oidc_jwt;
mod oidc_store;
mod smtp_session;
pub(crate) mod system_config;
mod tls;
mod totp;
/// re-export shim: `UserStore` + credential helpers moved to the shared
/// `mailrs-core` crate (S5.2f). Kept as `crate::users` so the web / imap /
/// pop3 / mcp / smtp call sites stay unchanged.
mod users {
    pub use mailrs_core::users::*;
}
mod web;

use bootstrap::*;

mod webhook;

use hickory_resolver::TokioResolver;

use crate::config::ServerConfig;
use crate::inbound::rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};
use crate::smtp_session::ConnectionContext;
use crate::users::UserStore;
use mailrs_mailbox::PgMailboxStore;
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

// Re-export only the event types integration tests need to observe the
// receiving pipeline. The driver itself lives in `test_support`, which
// builds a real ConnectionContext internally — so none of the heavy
// server types (ConnectionContext, WebState, …) have to become public
// API and trip `private_interfaces` once this crate compiles as a lib.
pub use event_bus::{BroadcastEvent, EventBus, SmtpEvent};

#[doc(hidden)]
pub mod test_support;

mod boot;
mod sieve_store;

/// Server entry point — every role this crate can play, which is the shape
/// this binary has always had. Boots config, all stores and every listener,
/// then blocks until a shutdown signal.
///
/// Lives in the library so both the `mailrs-server` binary and integration
/// tests share one crate root. The sequence itself is in `boot.rs`; the only
/// difference between this and `run_pg_core` is which roles are on.
pub async fn run() {
    boot::run_with_roles(boot::Roles::all()).await;
}

#[cfg(feature = "core-rpc")]
pub use pg_core_boot::run_pg_core;
