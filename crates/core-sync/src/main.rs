//! `mailrs-core-sync` — one-shot bidirectional mail-store migration
//! between two mailrs cores over the `mailrs-core-api` contract.
//!
//! Usage:
//!   mailrs-core-sync --from <SRC_RPC_BASE> --to <DST_RPC_BASE>
//!
//! Env:
//!   MAILRS_CORE_API_SECRET   bearer secret shared by both cores (required)
//!   MAILRS_CORE_SYNC_SECRET_FROM / _TO   optional per-side overrides
//!
//! Direction-agnostic: PG→kevy and kevy→PG are the same code path. Run at
//! switch time with both cores up (source read-only); after it completes,
//! flip `MAILRS_CORE_RPC_BASE` to the destination. The text index is
//! part of the kevy store, so it moves with the data.

use std::process::ExitCode;

use mailrs_core_api::client::Client;
use mailrs_core_sync::{SyncOpts, sync};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut from: Option<String> = None;
    let mut to: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--from" => from = args.next(),
            "--to" => to = args.next(),
            "-h" | "--help" => {
                eprintln!("usage: mailrs-core-sync --from <SRC_RPC_BASE> --to <DST_RPC_BASE>");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown arg: {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let (Some(from), Some(to)) = (from, to) else {
        eprintln!("both --from and --to are required");
        return ExitCode::FAILURE;
    };

    let base_secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV).unwrap_or_default();
    let from_secret = std::env::var("MAILRS_CORE_SYNC_SECRET_FROM").unwrap_or(base_secret.clone());
    let to_secret = std::env::var("MAILRS_CORE_SYNC_SECRET_TO").unwrap_or(base_secret);

    let src = Client::new(from.clone(), from_secret);
    let dst = Client::new(to.clone(), to_secret);

    // fail fast if either endpoint is unreachable
    if let Err(e) = src.readyz().await {
        eprintln!("source {from} not ready: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = dst.readyz().await {
        eprintln!("destination {to} not ready: {e}");
        return ExitCode::FAILURE;
    }

    tracing::info!(%from, %to, "core-sync starting");
    match sync(&src, &dst, &SyncOpts::default()).await {
        Ok(report) => {
            tracing::info!(
                accounts = report.accounts,
                aliases = report.aliases,
                threads = report.threads,
                delivered = report.messages_delivered,
                skipped_dupe = report.messages_skipped_dupe,
                "core-sync complete"
            );
            println!(
                "done: accounts={} aliases={} threads={} delivered={} skipped_dupe={}",
                report.accounts,
                report.aliases,
                report.threads,
                report.messages_delivered,
                report.messages_skipped_dupe
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("core-sync failed: {e}");
            ExitCode::FAILURE
        }
    }
}
