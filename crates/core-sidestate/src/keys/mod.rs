//! Network-kevy keys that more than one process reads or writes and that
//! have no handler family of their own here.
//!
//! A key spelled in two crates can drift, and then one side writes where
//! the other never reads, with no error on either side. Each key below is
//! spelled once; `scripts/check-shared-keys.sh` rejects a second spelling.

pub mod admin;
pub mod dmarc;
pub mod events;
pub mod push;
pub mod quota;
pub mod reset;
pub mod session;
pub mod signout;
