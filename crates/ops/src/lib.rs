//! `mailrs-ops`: the operations daemon.
//!
//! It runs beside the mail processes, reads fastcore and webapi over
//! HTTP, receives the signed reports every other process pushes, keeps
//! the state of each alert in its own SQLite file, and tells people through
//! channels that do not pass through this server's own mail path. It never
//! connects to kevy-server and never mounts the mail volume.

pub mod api;
pub mod app;
pub mod cli;
pub mod clock;
pub mod config;
pub mod daemon;
pub mod engine;
pub mod notify;
pub mod readings;
pub mod rules;
pub mod store;
