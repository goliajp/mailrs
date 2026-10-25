//! The IMAP, POP3 and ManageSieve servers of mailrs.
//!
//! Both cores — `mailrs-fastcore` over embedded kevy, `mailrs-server`
//! over SQL — answer these protocols through the same sessions here. A
//! core supplies its storage by implementing [`backend::Directory`] and
//! [`backend::Account`], and starts the listeners with
//! [`listen::spawn_all`].
//!
//! One crate rather than three: the protocols share the listener and TLS
//! wiring, the account model (POP3 is a view of the IMAP INBOX), and the
//! test client.

pub mod backend;
pub mod guard;
pub mod imap;
pub mod listen;
pub mod managesieve;
pub mod pop3;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod test_tls;

pub use guard::Guarded;
pub use listen::spawn_all;
