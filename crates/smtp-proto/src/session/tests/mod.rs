//! Session state-machine tests, split by subject on 2026-08-02.
//!
//! One 1,326-line file held all eighty-three. `file-size.md` excludes a
//! trailing inline `#[cfg(test)] mod tests` from a prod file's count but
//! binds the test block itself to the same 500 — and says the fix for an
//! oversized one is to split, not to hide it behind a filename.

mod auth;
mod capabilities;
mod flow;
mod helpers;
mod ordering;
mod reset;
