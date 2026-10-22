//! Session state-machine tests, one module per subject.
//!
//! One 1,326-line file held all eighty-three. The file-size limit excludes a
//! trailing inline `#[cfg(test)] mod tests` from a prod file's count but
//! binds the test block itself to the same 500 — and says the fix for an
//! oversized one is to split, not to hide it behind a filename.

mod auth;
mod capabilities;
mod flow;
mod helpers;
mod ordering;
mod pending_recipient;
mod reset;
