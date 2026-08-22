//! Tests for the conversation-list dispatcher, split by axis on
//! 2026-08-02 — 848 of the module's 1,021 lines were these three.
//!
//! Named modules, so `file-size.md` counts them: only a single trailing
//! `mod tests` is free, and three separate suites are not that.

#![cfg(test)]

mod account_filter;
mod archive_scope;
mod bucket_axis;
mod filters;
mod junk_cutover;
