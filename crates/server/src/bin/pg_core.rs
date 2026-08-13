//! `mailrs-pg-core` — the SQL core, fastcore's peer.
//!
//! Not the fat process `.claude/rfcs/20260722-monolith-out-of-image.md` ruled
//! out: same boot sequence as `mailrs-server`, a different role set. On are the
//! core-api contract, the spool drain that indexes arrivals, and the protocols
//! this side owns (IMAP, POP3, ManageSieve). Off are the four another process
//! owns — SMTP, the web tier, outbound delivery, the RBL monitor. Pointing
//! `webapi`'s `MAILRS_CORE_RPC_BASE` here instead of at `mailrs-fastcore` is the
//! switch.
//!
//! Everything it does lives in `mailrs_server::run_pg_core`, so `core_rpc` and
//! `boot` stay private and this binary reaches exactly one entry point.
//!
//! `required-features = ["core-rpc"]` in Cargo.toml: on the default axis this
//! target does not exist, so the everyday build is unchanged.
//!
//! **No tracing setup here.** `run_pg_core` shares `mailrs-server`'s boot
//! sequence, which installs the subscriber itself, and installing a second one
//! is a panic — `SetGlobalDefaultError`, on the first line of `main`, before
//! anything is logged. This binary did exactly that, and it took the image's own
//! startup check to notice: the workspace compiled, clippy was clean, 4,712
//! tests passed and the container built, because none of those run the process.
//! A binary that panics on startup passes every build check there is.
//! (`scripts/build-pg-core.sh` runs it once for this reason.)
//!
//! No `#[tokio::main]` either — the runtime is built by hand so the boot
//! sequence and this entry point cannot disagree about how many threads it has.

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(mailrs_server::run_pg_core());
}
