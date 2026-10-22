//! Per-message operations on [`crate::pg::PgMailboxStore`].
//!
//! Sub-modules:
//! - [`arrival`] — a delivered message's row and the facts beside it.
//! - [`index`] — insertion (`index_message`, `index_delivered`).
//! - [`read`] — lookup/list queries (`list_messages`, `get_message`,
//!   `query_messages`, `find_message_*`).
//! - [`mutate`] — state changes (`expunge`, `copy_message`,
//!   `move_message`, `update_message_content`, `update_invite_payload`).

mod arrival;
mod index;
mod mutate;
mod read;
mod uids;

pub use arrival::ArrivalFacts;
