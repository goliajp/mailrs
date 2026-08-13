//! `mailrs-pg-core` — the core-api contract, served from the SQL backend.
//!
//! The shape `.claude/rfcs/20260722-monolith-out-of-image.md` set when the
//! monolith left the image: not reviving the fat process, but running the
//! ~2,600 lines of `core_rpc` on their own while every protocol stays with the
//! process that already owns it. `webapi` is backend-agnostic — pointing its
//! `MAILRS_CORE_RPC_BASE` here instead of at `mailrs-fastcore` is the switch.
//!
//! Everything it does lives in `mailrs_server::run_pg_core`, so the `core_rpc`
//! module stays private and this binary reaches exactly one entry point.
//!
//! `required-features = ["core-rpc"]` in Cargo.toml: on the default axis this
//! target does not exist, so the everyday build is unchanged.

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(mailrs_server::run_pg_core());
}
