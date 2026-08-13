//! `mailrs-core-sync` — one-shot bidirectional mail-store migration
//! between two mailrs cores over the `mailrs-core-api` contract.
//!
//! Usage:
//!   mailrs-core-sync --from <SRC_RPC_BASE> --to <DST_RPC_BASE> [--dry-run]
//!                    [--page-size <N>]
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
    let mut dry_run = false;
    let mut page_size: Option<u32> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--from" => from = args.next(),
            "--to" => to = args.next(),
            "--dry-run" => dry_run = true,
            "--page-size" => {
                page_size = match args.next().as_deref().map(str::parse) {
                    Some(Ok(n)) if n > 0 => Some(n),
                    _ => {
                        eprintln!("--page-size needs a positive integer");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "-h" | "--help" => {
                eprintln!(
                    "usage: mailrs-core-sync --from <SRC> --to <DST> [--dry-run] \
                     [--page-size N]\n\
                     \n\
                     --dry-run   read both sides, write nothing, report what a real\n\
                     \x20            run would move. Do this first: a large difference\n\
                     \x20            is usually a backfill gap rather than a defect,\n\
                     \x20            and the way to tell is to look at what it consists\n\
                     \x20            of before acting on it.\n\
                     --page-size enumeration page size (default 200). Raise it if a run\n\
                     \x20            refuses because one second holds more threads than\n\
                     \x20            a page."
                );
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

    let opts = SyncOpts {
        dry_run,
        ..page_size
            .map(|n| SyncOpts {
                page_size: n,
                ..SyncOpts::default()
            })
            .unwrap_or_default()
    };

    tracing::info!(%from, %to, dry_run, "core-sync starting");
    match sync(&src, &dst, &opts).await {
        Ok(report) => {
            tracing::info!(
                accounts = report.accounts,
                aliases = report.aliases,
                threads = report.threads,
                delivered = report.messages_delivered,
                skipped_dupe = report.messages_skipped_dupe,
                "core-sync complete"
            );
            // Two verbs, because the same numbers mean different things: a real
            // run reports what it did, a dry run what it would do.
            let verb = if dry_run { "would move" } else { "moved" };
            println!(
                "{verb}: accounts={} aliases={} threads={} messages={} \
                 already_present={}",
                report.accounts,
                report.aliases,
                report.threads,
                report.messages_delivered,
                report.messages_skipped_dupe
            );
            // The gap, separated from the total. "threads examined" cannot come
            // out zero and so is not a measurement of anything; "threads whose
            // message set already matches" can.
            println!(
                "  of those threads, {} already match on both sides ({} differ)",
                report.threads_already_identical,
                report.threads - report.threads_already_identical,
            );
            if dry_run {
                println!(
                    "  accounts only on the destination: {}",
                    report.accounts_only_on_dst
                );
                if report.accounts_only_on_dst > 0 {
                    println!(
                        "  NOTE: those accounts exist on the destination and not the \
                         source. A one-directional copy will not remove them, so after \
                         a switch their mail is readable on one core and not the other."
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("core-sync failed: {e}");
            ExitCode::FAILURE
        }
    }
}
