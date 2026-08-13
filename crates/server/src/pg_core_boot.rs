//! The `mailrs-pg-core` entry point — fastcore's peer on a SQL backend.
//!
//! `pub(crate)` on the module, `pub` on the one function: the binary in
//! `src/bin/pg_core.rs` is a separate crate and can only reach `pub` items,
//! and this is the only one it needs. `boot` and `core_rpc` stay private.

#![cfg(feature = "core-rpc")]

/// Boot the SQL core as the peer of `mailrs-fastcore`.
///
/// **This used to boot the contract and nothing else**, on the reading that
/// the roles wrapped around `core_rpc/` all have their own processes in the
/// fastcore stack so none of them come along. That is true of four of them and
/// false of the rest, and the difference matters: fastcore and this process are
/// two branches of one switch, not a front and a store behind it. Either one
/// runs, whole. A contract-only process would have served every read the web UI
/// makes and indexed no arriving mail — receiver spools, and on this side
/// nothing would have drained it, so the inbox would simply stop growing.
/// (`fastcore`'s own `MAILRS_CORE_RPC_BASE` runs the other way: it *polls* a
/// remote core and mirrors threads in, the monolith-era cutover path. There has
/// never been a way for fastcore to write into another core.)
///
/// So this is `run_with_roles` with the peer's role set, over the same boot
/// sequence `run()` uses. On:
///
/// - the core-api contract, which webapi's `MAILRS_CORE_RPC_BASE` points at;
/// - the spool drain, so arrivals index into this backend;
/// - IMAP / IMAPS / POP3 / POP3S / ManageSieve — mailbox contents come off the
///   shared maildir either way, so what this side supplies is the credential
///   check and the uid/flag bookkeeping;
/// - the periodic subsystems both cores want: webhook delivery, TLS-RPT,
///   calendar feeds, DMARC aggregate reports.
///
/// Off, because another process owns each: SMTP (`mailrs-receiver`), the web
/// tier (`mailrs-webapi`), outbound delivery (`mailrs-fastcore-sender`), and
/// the RBL monitor. Selecting roles is the whole difference between this and
/// the fat process `.claude/rfcs/20260722-monolith-out-of-image.md` ruled out —
/// that one's defining property was starting all of them at once.
///
/// Env: everything `ServerConfig::from_env()` reads, which includes
/// `MAILRS_PG_URL` and `MAILRS_MAILDIR`, plus `MAILRS_KEVY_URL` for the shared
/// side-state families and the two `spawn_core_rpc` reads itself,
/// `MAILRS_CORE_RPC_ADDR` and `MAILRS_CORE_API_SECRET`.
pub async fn run_pg_core() {
    crate::boot::run_with_roles(crate::boot::Roles::peer()).await;
}
