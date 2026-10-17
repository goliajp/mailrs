//! Disposable test mailboxes: the operations behind `/api/mock/*` and the
//! `mock_*` MCP tools, so the two surfaces cannot disagree.
//!
//! Mail is read here and written by fastcore's spool drain; both go through
//! `mailrs_core_sidestate::families::mock`, which owns the keys and paths.

pub mod messages;
pub mod rest;
pub mod send;
pub mod service;
