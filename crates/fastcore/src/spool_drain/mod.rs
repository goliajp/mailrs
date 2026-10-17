//! Spool drain — fastcore side of the receiver/core split.
//!
//! In split topology the receiver process accepts SMTP and writes
//! `{spool_root}/incoming/{new,cur}/*` files with an
//! `X-Mailrs-Spool-Envelope` header prepended (base64 JSON with
//! reverse_path + forward_paths + verdict). The monolith `core` used to
//! poll that dir and hand each file to its resolve/sieve/deliver
//! pipeline; nothing owned this in the fastcore split — inbound mail
//! landed in the spool and stayed there. This module closes that gap:
//! decode the envelope, strip the header, and drop the file into
//! `{maildir_root}/<domain>/<local>/new/`. Fastcore's periodic
//! maildir self-heal (see `healed_from_maildir` in `lib.rs`) then
//! threads and indexes it.
//!
//! A recipient resolves to its own maildir, a fresh account's, an email
//! group's members, or an alias target's (`route`). Files with no
//! resolvable recipient are left in the spool with a warn log so a human
//! can look, and set aside after a week (`aging`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mailrs_core::spool::decode_spool_blob;

use crate::FastcoreState;

mod aging;
mod check;
mod deliver;
mod email_group;
mod mock;
mod recipient;
pub mod relay;
mod reported_to;
mod route;
mod stuck;

pub(crate) use check::recipient_deliverable_route;
pub(crate) use deliver::*;
use reported_to::reported_to;
use stuck::{forget_departed, stuck_is_new};

/// Spawn the drain loop. Env:
/// - `MAILRS_SPOOL_ROOT` — spool base, default `/data/.spool`
/// - `MAILRS_MAILDIR`    — maildir root, default `/data/maildir`
/// - `MAILRS_FASTCORE_SPOOL_INTERVAL_SECS` — poll interval, default 15
///
/// If the spool `incoming/` dir doesn't exist yet, the loop still runs
/// — receiver may not have written its first file. Missing-dir errors
/// downgrade to debug.
pub async fn spawn(state: Arc<FastcoreState>) {
    let spool_root =
        std::env::var("MAILRS_SPOOL_ROOT").unwrap_or_else(|_| "/data/.spool".to_string());
    let maildir_root =
        std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".to_string());
    let interval_secs: u64 = std::env::var("MAILRS_FASTCORE_SPOOL_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    let incoming_new = PathBuf::from(&spool_root).join("incoming").join("new");
    let incoming_cur = PathBuf::from(&spool_root).join("incoming").join("cur");
    tracing::info!(
        spool_new = %incoming_new.display(),
        spool_cur = %incoming_cur.display(),
        maildir_root = %maildir_root,
        interval_secs,
        "fastcore spool drain started"
    );
    // No idle backoff here, deliberately — unlike `calendar_sync` and
    // `webhook_delivery`, which both use `idle_backoff`.
    //
    // This interval is not polling overhead, it is the delivery latency
    // budget: mail sits in the spool until a tick picks it up, so doubling
    // the wait doubles how long a quiet mailbox takes to show new mail, and
    // a mailbox is quiet precisely when someone is waiting for the first
    // message. An empty tick costs two readdirs on two empty directories.
    //
    // Checked on 2026-08-01 against the rule that periodic work must
    // converge: the rule is about loops whose resting state is expensive, and this one's is
    // not. Written down because this loop looks like the violation
    // `calendar_sync` actually was.
    loop {
        let (delivered_new, seen_new) = drain_once(&incoming_new, &maildir_root, &state);
        let (delivered_cur, seen_cur) = drain_once(&incoming_cur, &maildir_root, &state);
        let mut seen_all = seen_new;
        seen_all.extend(seen_cur);
        forget_departed(&seen_all);
        let total = delivered_new + delivered_cur;
        if total > 0 {
            tracing::info!(delivered = total, "fastcore spool drain tick");
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// Walk one spool dir once, deliver every decodable file to its
/// recipient maildir(s), and remove it. Returns delivered count and
/// every filename walked — the caller unions the counts across the
/// `new` and `cur` dirs before expiring stuck-file reports, because a
/// per-dir expiry would let the empty dir forget the other's files and
/// re-warn them on the very next tick.
pub fn drain_once(
    dir: &Path,
    maildir_root: &str,
    state: &Arc<FastcoreState>,
) -> (usize, std::collections::HashSet<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(dir = %dir.display(), error = %e, "spool dir read");
            }
            return (0, std::collections::HashSet::new());
        }
    };
    let mut delivered = 0;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        seen.insert(filename.clone());
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "spool read");
                continue;
            }
        };
        let (env, body) = match decode_spool_blob(&bytes) {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "spool decode; skipping");
                continue;
            }
        };
        if let Some(stored) = mock::drain_sandbox(state, &env, &filename, &path, body) {
            delivered += usize::from(stored);
            continue;
        }
        let inbound = recipient::Inbound {
            state,
            maildir_root,
            filename: &filename,
            body,
            env: &env,
        };
        let mut delivered_here = 0usize;
        let mut unresolved: Vec<String> = Vec::new();
        for (rcpt_index, raw_fwd) in env.forward_paths.iter().enumerate() {
            // Fold once, before anything is looked up. Maildir paths and
            // both kevy key families are written lowercase, so an
            // uppercase RCPT TO used verbatim misses all three and the
            // message is accepted at SMTP and then never delivered — one
            // file sat here from 2026-08-06 to 2026-08-14 for exactly
            // that reason. Everything downstream (delivery, sieve, DSN,
            // logging) gets the canonical form too, so the address a
            // human reads in a log is the one the lookups used.
            let fwd = &canonical_recipient(raw_fwd);
            let from = mock::Sender::of(&env);
            match mock::divert(state, from, fwd, rcpt_index, &filename, body) {
                mock::Diverted::No => {}
                mock::Diverted::Done => {
                    delivered_here += 1;
                    continue;
                }
                mock::Diverted::Retry => {
                    unresolved.push(fwd.clone());
                    continue;
                }
            }
            if relay::relays_outward(env.is_authenticated, fwd, &relay::local_domains(state)) {
                match relay::enqueue_submission(&env.reverse_path, raw_fwd, body) {
                    Ok(()) => delivered_here += 1,
                    Err(e) => {
                        tracing::warn!(to = %raw_fwd, error = %e, "submission relay enqueue failed");
                        unresolved.push(fwd.clone());
                    }
                }
                continue;
            }
            let copies = inbound.deliver_to(raw_fwd, fwd);
            delivered_here += copies.delivered;
            unresolved.extend(copies.unresolved);
        }
        if delivered_here > 0 {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    delivered += 1;
                    tracing::info!(
                        file = %filename,
                        from = %env.reverse_path,
                        to = ?env.forward_paths,
                        "spool → maildir"
                    );
                }
                Err(e) => tracing::warn!(
                    path = %path.display(), error = %e,
                    "spool remove after deliver"
                ),
            }
            if !unresolved.is_empty() {
                tracing::warn!(
                    file = %filename,
                    unresolved = ?unresolved,
                    "spool delivered to some recipients only"
                );
            }
        } else if !aging::set_aside_if_stale(dir, &path, &filename) && stuck_is_new(&filename) {
            // Warn once per file, not once per tick. This fires every
            // drain interval otherwise, and three genuinely undeliverable
            // messages spent 11 days reprinting the same two lines every
            // 15 s until the signal was indistinguishable from noise
            // (2026-07-20). The file still stays put for a human.
            tracing::warn!(
                file = %filename,
                fwd_paths = ?env.forward_paths,
                "spool file has no resolvable recipient; leaving in place"
            );
        }
    }
    (delivered, seen)
}
