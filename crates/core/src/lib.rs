//! mailrs-core — shared session infrastructure.
//!
//! Types both the stateful server core and the stateless receiver need but
//! that belong to neither alone: the [`SmtpEvent`] bus today, and
//! incrementally the cross-protocol session primitives (auth store, TLS
//! state, LDAP config). Free of spg / kevy so it sits below both.

pub mod event_bus;
pub mod ldap_auth;
pub mod users;

pub use event_bus::{BroadcastEvent, EventBus, EventPublisher, SmtpEvent, next_connection_id};
