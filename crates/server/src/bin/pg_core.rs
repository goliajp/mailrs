//! `mailrs-pg-core` — the SQL core, fastcore's peer.
//!
//! It serves the core-api contract, drains the spool the receiver writes and
//! indexes each arrival, and runs IMAP, POP3 and ManageSieve. SMTP, the web
//! tier and outbound delivery belong to `mailrs-receiver`, `mailrs-webapi` and
//! `mailrs-fastcore-sender`. Pointing `webapi`'s `MAILRS_CORE_RPC_BASE` here
//! instead of at `mailrs-fastcore` is the switch.
//!
//! Everything it does lives in `mailrs_server::run_pg_core`, so the boot and
//! `core_rpc` stay private and this binary reaches exactly one entry point.
//!
//! **No tracing setup here.** The boot installs the subscriber itself, and
//! installing a second one is a panic — `SetGlobalDefaultError`, on the first
//! line of `main`, before anything is logged. A binary that panics on startup
//! passes every build check there is, which is why `scripts/build-pg-core.sh`
//! runs it once.
//!
//! No `#[tokio::main]` either — the runtime is built by hand so the boot
//! sequence and this entry point cannot disagree about how many threads it has.

fn main() {
    mailrs_core_api::refuse_blank_secrets();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(mailrs_server::run_pg_core());
}
