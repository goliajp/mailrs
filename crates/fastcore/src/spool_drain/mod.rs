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
//! Recipient resolution is direct-mailbox only in v1 — no alias table
//! yet (that's a follow-up gap). Files with no resolvable recipient are
//! left in the spool with a warn log so a human can look.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mailrs_core::spool::decode_spool_blob;

use crate::FastcoreState;

mod deliver;

pub(crate) use deliver::*;

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
    // Checked against periodic-work-must-converge on 2026-08-01: the rule
    // is about loops whose resting state is expensive, and this one's is
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

/// Filenames already reported as undeliverable. A spool file that
/// cannot be resolved stays on disk by design, so without this every
/// drain tick would re-log it forever.
static STUCK_REPORTED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// `true` the first time `filename` is seen as stuck. Also drops names
/// that have since left the spool, so a file that is fixed and then
/// breaks again is reported afresh.
fn stuck_is_new(filename: &str) -> bool {
    match STUCK_REPORTED.lock() {
        Ok(mut set) => set.insert(filename.to_string()),
        // A poisoned mutex must not silence the warning.
        Err(_) => true,
    }
}

/// Forget any reported name no longer present in the spool.
fn forget_departed(present: &std::collections::HashSet<String>) {
    if let Ok(mut set) = STUCK_REPORTED.lock() {
        set.retain(|name| present.contains(name));
    }
}

/// Walk one spool dir once, deliver every decodable file to its
/// recipient maildir(s), and remove it. Returns delivered count and
/// every filename walked — the caller unions the counts across the
/// `new` and `cur` dirs before expiring stuck-file reports, because a
/// per-dir expiry would let the empty dir forget the other's files and
/// re-warn them on the very next tick.
pub(crate) fn drain_once(
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
        let mut delivered_here = 0usize;
        let mut unresolved: Vec<String> = Vec::new();
        for raw_fwd in &env.forward_paths {
            // Fold once, before anything is looked up. Maildir paths and
            // both kevy key families are written lowercase, so an
            // uppercase RCPT TO used verbatim misses all three and the
            // message is accepted at SMTP and then never delivered — one
            // file sat here from 2026-08-06 to 2026-08-14 for exactly
            // that reason. Everything downstream (delivery, sieve, DSN,
            // logging) gets the canonical form too, so the address a
            // human reads in a log is the one the lookups used.
            let fwd = &canonical_recipient(raw_fwd);
            // 1. Resolve the addressable recipient — direct maildir, or
            //    the alias table (`mailrs:alias:<addr>` → target). Both
            //    cases return the address we'll actually deliver TO.
            let resolved_addr: Option<String> = if has_maildir(maildir_root, fwd) {
                Some(fwd.clone())
            } else if provision_if_account(state, maildir_root, fwd) {
                // account exists in kevy but was never provisioned a
                // maildir (fresh add_account) — create the skeleton so
                // its very first mail is deliverable
                Some(fwd.clone())
            } else {
                let via_alias = state.alias_store.resolve(fwd).ok().flatten();
                via_alias.and_then(|a| {
                    let a = canonical_recipient(&a);
                    if has_maildir(maildir_root, &a)
                        || provision_if_account(state, maildir_root, &a)
                    {
                        tracing::info!(orig = %fwd, aliased = %a, "spool alias resolved");
                        Some(a)
                    } else {
                        None
                    }
                })
            };
            // SRS reverse (G6): a bounce addressed to
            // SRS0=...@<our-domain> is the return path for mail we
            // forwarded. Reverse it to the original sender and relay
            // the bounce outward — never deliver it locally.
            //
            // This one reads `raw_fwd`, not the folded form. An SRS local
            // part is `SRS0=<hmac>=<tt>=<domain>=<local>`, and both halves
            // of it are case-sensitive: `reverse` strips the literal
            // `SRS0=` prefix, and the HMAC is computed over the domain and
            // local part it carries. Folding the address would make the
            // prefix strip fail and, past that, produce a different hash —
            // so bounces for mail we forwarded would stop being relayed and
            // would instead land in `unresolved` and sit in the spool. That
            // is the same defect this fold exists to remove, so the fold
            // must not reach here.
            if raw_fwd.to_ascii_uppercase().starts_with("SRS0=")
                && let Ok(secret) = std::env::var("MAILRS_SRS_SECRET")
                && let Some(original) =
                    mailrs_srs::reverse(raw_fwd, &secret, mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS)
            {
                match enqueue_outbound(&original, "", body) {
                    Ok(()) => {
                        tracing::info!(srs = %raw_fwd, %original, "SRS bounce relayed");
                        delivered_here += 1;
                    }
                    Err(e) => tracing::warn!(srs = %raw_fwd, error = %e, "SRS relay failed"),
                }
                continue;
            }
            let Some(addr) = resolved_addr else {
                unresolved.push(fwd.clone());
                continue;
            };
            // 1b. DMARC aggregate reports sent to the collector mailbox
            //     are parsed and stored here. Delivery is unaffected —
            //     the report still lands in the mailbox as ordinary
            //     mail, and every failure inside is logged and
            //     swallowed. Non-collector recipients short-circuit on
            //     the address check before any MIME work happens.
            crate::dmarc_ingest::maybe_ingest(&addr, body);
            // 1c. Feedback-loop complaints to abuse@ / postmaster@ take
            //     the complainant off the sending list. Same shape: a
            //     side effect, never a filter.
            crate::fbl::maybe_record_complaint(maildir_root, &addr, body);
            // 1d. TLS-RPT reports (RFC 8460) to the tlsrpt collector.
            crate::tlsrpt_ingest::maybe_ingest(&addr, body);
            // 2. Consult the recipient's sieve script. Actions map to a
            //    Decision that overrides the default INBOX write.
            let outcome = crate::sieve_apply::decide(&addr, body, Some(&env.reverse_path));
            // vacation fires only after a successful LOCAL delivery below
            let mut delivered_locally = false;
            match outcome.decision {
                crate::sieve_apply::Decision::Discard => {
                    delivered_here += 1;
                    tracing::info!(recipient = %addr, "sieve: discard");
                }
                crate::sieve_apply::Decision::Reject(reason) => {
                    // Backscatter guard: DSN only when the receiver's
                    // antispam verdict routed to INBOX (auth-scored OK
                    // proxy — the envelope carries no raw SPF/DKIM
                    // result) AND the sender is a real address. Anything
                    // else is silently consumed.
                    let allow = env.target_folder.eq_ignore_ascii_case("INBOX")
                        && !crate::bounce::suppress_bounce(&env.reverse_path);
                    if allow {
                        let (reporting_mta, from_domain) = crate::bounce::dsn_identity();
                        let dsn = crate::bounce::compose_dsn(
                            &reporting_mta,
                            &from_domain,
                            &env.reverse_path,
                            &addr,
                            "5.7.1",
                            &format!("550 5.7.1 {reason}"),
                            body,
                        );
                        match enqueue_outbound(&env.reverse_path, "", &dsn) {
                            Ok(()) => tracing::info!(recipient = %addr, "sieve: reject DSN queued"),
                            Err(e) => tracing::warn!(recipient = %addr, error = %e,
                                "sieve: reject DSN enqueue failed; message discarded"),
                        }
                    } else {
                        tracing::info!(recipient = %addr, "sieve: reject suppressed (backscatter guard)");
                    }
                    delivered_here += 1;
                }
                crate::sieve_apply::Decision::Redirect(target) => {
                    match enqueue_redirect(&addr, &target, body, &env.reverse_path) {
                        Ok(()) => {
                            delivered_here += 1;
                            tracing::info!(recipient = %addr, %target, "sieve: redirect");
                        }
                        Err(e) => {
                            tracing::warn!(recipient = %addr, %target, error = %e,
                                "sieve: redirect enqueue failed; falling back to Keep");
                            match deliver(maildir_root, &addr, "", &filename, body) {
                                Ok(true) => {
                                    delivered_here += 1;
                                    delivered_locally = true;
                                    crate::ingest_delivered_file(
                                        state,
                                        &addr,
                                        &filename,
                                        body,
                                        &env.target_folder,
                                    );
                                }
                                _ => unresolved.push(addr.clone()),
                            }
                        }
                    }
                }
                crate::sieve_apply::Decision::FileInto(folder) => {
                    let subfolder = crate::sieve_apply::maildir_subfolder(&folder);
                    match deliver(maildir_root, &addr, &subfolder, &filename, body) {
                        Ok(true) => {
                            delivered_here += 1;
                            delivered_locally = true;
                            tracing::info!(recipient = %addr, %subfolder, "sieve: fileinto");
                            let blob_ref = format!("{subfolder}/{filename}");
                            crate::ingest_delivered_file(state, &addr, &blob_ref, body, &folder);
                        }
                        Ok(false) => {
                            tracing::warn!(recipient = %addr, %subfolder,
                                "sieve: fileinto target dir missing; falling back to INBOX");
                            if let Ok(true) = deliver(maildir_root, &addr, "", &filename, body) {
                                delivered_here += 1;
                                delivered_locally = true;
                                crate::ingest_delivered_file(
                                    state,
                                    &addr,
                                    &filename,
                                    body,
                                    &env.target_folder,
                                );
                            } else {
                                unresolved.push(addr.clone());
                            }
                        }
                        Err(e) => {
                            tracing::warn!(recipient = %addr, error = %e,
                                "sieve: fileinto write failed; falling back to INBOX");
                            if let Ok(true) = deliver(maildir_root, &addr, "", &filename, body) {
                                delivered_here += 1;
                                delivered_locally = true;
                                crate::ingest_delivered_file(
                                    state,
                                    &addr,
                                    &filename,
                                    body,
                                    &env.target_folder,
                                );
                            } else {
                                unresolved.push(addr.clone());
                            }
                        }
                    }
                }
                crate::sieve_apply::Decision::Keep => {
                    match deliver(maildir_root, &addr, "", &filename, body) {
                        Ok(true) => {
                            delivered_here += 1;
                            delivered_locally = true;
                            crate::ingest_delivered_file(
                                state,
                                &addr,
                                &filename,
                                body,
                                &env.target_folder,
                            );
                        }
                        Ok(false) => unresolved.push(addr.clone()),
                        Err(e) => {
                            tracing::warn!(fwd = %addr, error = %e, "spool deliver");
                            unresolved.push(addr.clone());
                        }
                    }
                }
            }
            if delivered_locally && let Some(vac) = outcome.vacation {
                crate::sieve_apply::maybe_vacation_reply(&addr, &env.reverse_path, body, &vac);
            }
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
        } else if stuck_is_new(&filename) {
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
