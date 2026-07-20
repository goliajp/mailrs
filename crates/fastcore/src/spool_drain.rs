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
fn drain_once(
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
        for fwd in &env.forward_paths {
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
            if fwd.to_ascii_uppercase().starts_with("SRS0=")
                && let Ok(secret) = std::env::var("MAILRS_SRS_SECRET")
                && let Some(original) =
                    mailrs_srs::reverse(fwd, &secret, mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS)
            {
                match enqueue_outbound(&original, "", body) {
                    Ok(()) => {
                        tracing::info!(srs = %fwd, %original, "SRS bounce relayed");
                        delivered_here += 1;
                    }
                    Err(e) => tracing::warn!(srs = %fwd, error = %e, "SRS relay failed"),
                }
                continue;
            }
            let Some(addr) = resolved_addr else {
                unresolved.push(fwd.clone());
                continue;
            };
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
                        let helo = std::env::var("MAILRS_HELO_HOSTNAME")
                            .unwrap_or_else(|_| "mailrs".into());
                        let dsn = crate::bounce::compose_dsn(
                            &helo,
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

/// Deliver one file to one recipient. `subfolder` is empty for INBOX
/// or `.Maildir++Folder` (produced by sieve_apply::maildir_subfolder)
/// for a fileinto action. Returns `Ok(true)` on success, `Ok(false)`
/// when the target directory is absent.
fn deliver(
    maildir_root: &str,
    addr: &str,
    subfolder: &str,
    filename: &str,
    body: &[u8],
) -> std::io::Result<bool> {
    let (local, domain) = match addr.split_once('@') {
        Some(x) => x,
        None => return Ok(false),
    };
    let base = PathBuf::from(maildir_root).join(domain).join(local);
    let user_new = if subfolder.is_empty() {
        base.join("new")
    } else {
        // Auto-create the Maildir++ subfolder skeleton on first fileinto,
        // matching what an IMAP client would do via CREATE — otherwise a
        // freshly-provisioned account can't receive filed mail.
        let sub = base.join(subfolder);
        for leaf in ["cur", "new", "tmp"] {
            let _ = std::fs::create_dir_all(sub.join(leaf));
        }
        sub.join("new")
    };
    if !user_new.is_dir() {
        return Ok(false);
    }
    let target = user_new.join(filename);
    std::fs::write(&target, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));
    }
    Ok(true)
}

/// A kevy account with no maildir gets one created on first delivery.
/// Returns true when the address is a known account and the skeleton
/// now exists. New-account provisioning (add_account RPC) never made
/// the maildir, so a fresh account's first inbound mail sat unresolved
/// in the spool forever.
fn provision_if_account(state: &Arc<FastcoreState>, maildir_root: &str, addr: &str) -> bool {
    let Ok(Some(_)) = state.mailbox.get_account_blob(addr) else {
        return false;
    };
    let Some((local, domain)) = addr.split_once('@') else {
        return false;
    };
    let base = PathBuf::from(maildir_root).join(domain).join(local);
    for leaf in ["cur", "new", "tmp"] {
        if std::fs::create_dir_all(base.join(leaf)).is_err() {
            return false;
        }
    }
    tracing::info!(%addr, "provisioned maildir for kevy account on first delivery");
    true
}

/// Quick recipient-existence probe used before choosing between direct
/// delivery and alias resolution. Splits `addr@dom`, checks the
/// per-user `new/` dir exists. Returns false for malformed addresses.
fn has_maildir(maildir_root: &str, addr: &str) -> bool {
    let Some((local, domain)) = addr.split_once('@') else {
        return false;
    };
    PathBuf::from(maildir_root)
        .join(domain)
        .join(local)
        .join("new")
        .is_dir()
}

/// Enqueue an arbitrary outbound message (DSN / auto-reply). `sender`
/// is the MAIL FROM ("<>" for null-envelope notifications).
fn enqueue_outbound(to: &str, sender: &str, body: &[u8]) -> std::io::Result<()> {
    let mail_from = if sender.is_empty() { "<>" } else { sender };
    enqueue_redirect(mail_from, to, body, mail_from)
}

/// Push a sieve `redirect` action into the outbound queue.
///
/// Wire shape matches `mailrs-outbound-queue`'s existing envelope so
/// `mailrs-fastcore-sender` picks it up without special-casing:
/// - LPUSH `mailrs:outbound:pending`  <id>
/// - HSET  `mailrs:outbound:<id>`     blob = JSON envelope
///
/// `id` is a millisecond timestamp + a random suffix so concurrent
/// redirects on the same tick don't collide.
fn enqueue_redirect(
    original_recipient: &str,
    target: &str,
    body: &[u8],
    reverse_path: &str,
) -> std::io::Result<()> {
    use base64::Engine as _;
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        return Err(std::io::Error::other("MAILRS_KEVY_URL unset"));
    };
    let mut conn = kevy_client::Connection::open(&url).map_err(std::io::Error::other)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let nonce: u32 = {
        // Cheap non-crypto uniqueness — a millisecond suffix is enough
        // when redirects fire from the same drain tick.
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    let id = format!("{now_ms}-{nonce}");
    let b64_body = base64::engine::general_purpose::STANDARD.encode(body);
    // SRS forward-rewrite the MAIL FROM (G6): when we forward on behalf
    // of an external sender, the receiving MX runs SPF against OUR IP —
    // an un-rewritten foreign reverse-path fails SPF and the forward is
    // dropped. SRS0=...@<our-domain> is SPF-aligned to us and reverses
    // so bounces route back to the original sender. Null senders (system
    // notifications) and same-domain senders stay as-is.
    let mail_from = match std::env::var("MAILRS_SRS_SECRET").ok() {
        Some(secret) if !reverse_path.trim().trim_matches(['<', '>']).is_empty() => {
            let our_domain = original_recipient.split('@').nth(1).unwrap_or("");
            let rp = reverse_path.trim().trim_matches(['<', '>']);
            if rp.split('@').nth(1) == Some(our_domain) {
                rp.to_string() // already our domain — SPF-aligned
            } else {
                mailrs_srs::rewrite(rp, our_domain, &secret)
            }
        }
        _ => original_recipient.to_string(),
    };
    // NOTE `recipient` singular — the sender process reads that exact
    // field; the earlier `recipients` array made every redirect
    // envelope land in move_to_failed as "malformed"
    let envelope = serde_json::json!({
        "sender": mail_from,
        "recipient": target,
        "message_data_b64": b64_body,
        "attempts": 0,
        "next_attempt": 0,
        "id": &id,
        "envelope_from": reverse_path,
    });
    let blob = envelope.to_string();
    let hash_key = format!("mailrs:outbound:{id}");
    conn.hset(
        hash_key.as_bytes(),
        &[(b"blob".as_slice(), blob.as_bytes())],
    )
    .map_err(std::io::Error::other)?;
    conn.lpush(b"mailrs:outbound:pending", &[id.as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
    use mailrs_mailbox_kevy::KevyMailboxStore;

    fn state() -> Arc<FastcoreState> {
        let store = Arc::new(Store::open(Config::default()).unwrap());
        let mailbox = KevyMailboxStore::new(store);
        Arc::new(FastcoreState::new(mailbox))
    }

    fn envelope(forward_paths: &[&str]) -> SpoolEnvelope {
        SpoolEnvelope {
            reverse_path: "alice@example.com".into(),
            forward_paths: forward_paths.iter().map(|s| s.to_string()).collect(),
            is_authenticated: false,
            conn_id: 1,
            target_folder: "INBOX".into(),
            received_at: 42,
            schema_version: SPOOL_SCHEMA_VERSION,
        }
    }

    fn setup_user_maildir(root: &std::path::Path, addr: &str) -> std::path::PathBuf {
        let (local, domain) = addr.split_once('@').unwrap();
        let base = root.join(domain).join(local);
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
        }
        base
    }

    #[test]
    fn drain_moves_file_and_strips_envelope() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        let user_base = setup_user_maildir(&maildir_root, "bob@example.com");

        let body = b"From: alice@example.com\r\nSubject: hi\r\n\r\nhello\r\n";
        let blob = encode_spool_blob(&envelope(&["bob@example.com"]), body);
        let spool_file = spool_new.join("1000000.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let delivered = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 1);
        assert!(!spool_file.exists());
        let landed = user_base.join("new").join("1000000.M1P1Q1.host");
        assert_eq!(std::fs::read(&landed).unwrap(), body);
    }

    #[test]
    fn drain_leaves_file_when_no_recipient_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();

        let blob = encode_spool_blob(&envelope(&["ghost@example.com"]), b"body");
        let spool_file = spool_new.join("1000001.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let delivered = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 0);
        assert!(spool_file.exists(), "unresolved file must stay in spool");
    }

    #[test]
    fn drain_delivers_partial_and_removes_when_any_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        let bob_base = setup_user_maildir(&maildir_root, "bob@example.com");

        let blob = encode_spool_blob(
            &envelope(&["bob@example.com", "ghost@example.com"]),
            b"body",
        );
        let spool_file = spool_new.join("1000002.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let delivered = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 1);
        assert!(!spool_file.exists());
        assert!(bob_base.join("new").join("1000002.M1P1Q1.host").exists());
    }

    #[test]
    fn drain_skips_undecodable_file() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();

        let bogus = spool_new.join("garbage");
        std::fs::write(&bogus, b"not a spool envelope").unwrap();

        let delivered = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
        assert_eq!(delivered, 0);
        assert!(bogus.exists());
    }

    #[test]
    fn drain_returns_zero_on_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope").join("incoming").join("new");
        let maildir_root = tmp.path().join("maildir");
        std::fs::create_dir_all(&maildir_root).unwrap();
        assert_eq!(
            drain_once(&missing, maildir_root.to_str().unwrap(), &state()),
            0
        );
    }

    #[test]
    fn drain_falls_back_to_alias_when_direct_mailbox_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_new = tmp.path().join("spool").join("incoming").join("new");
        std::fs::create_dir_all(&spool_new).unwrap();
        let maildir_root = tmp.path().join("maildir");
        let alice_base = setup_user_maildir(&maildir_root, "alice@example.com");

        let s = state();
        s.mailbox
            .upsert_alias("contact@example.com", "alice@example.com")
            .unwrap();

        let blob = encode_spool_blob(&envelope(&["contact@example.com"]), b"aliased body");
        let spool_file = spool_new.join("1000003.M1P1Q1.host");
        std::fs::write(&spool_file, &blob).unwrap();

        let delivered = drain_once(&spool_new, maildir_root.to_str().unwrap(), &s);
        assert_eq!(delivered, 1);
        assert!(!spool_file.exists());
        assert!(
            alice_base.join("new").join("1000003.M1P1Q1.host").exists(),
            "aliased delivery must land in the resolved user's maildir"
        );
    }

    #[test]
    fn srs_roundtrip_reverses_to_original() {
        // forward-rewrite an external sender through our domain, then
        // reverse it back — this is the exact path the redirect MAIL
        // FROM + bounce-return use (G6)
        let secret = "test-secret";
        let rewritten = mailrs_srs::rewrite("bob@remote.example", "golia.jp", secret);
        assert!(rewritten.to_ascii_uppercase().starts_with("SRS0="));
        assert!(rewritten.ends_with("@golia.jp"));
        let back = mailrs_srs::reverse(
            &rewritten,
            secret,
            mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS,
        );
        assert_eq!(back.as_deref(), Some("bob@remote.example"));
        // wrong secret must fail verification
        assert!(mailrs_srs::reverse(&rewritten, "other", 14).is_none());
    }
}
