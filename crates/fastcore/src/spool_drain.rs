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
        let delivered_new = drain_once(&incoming_new, &maildir_root, &state);
        let delivered_cur = drain_once(&incoming_cur, &maildir_root, &state);
        let total = delivered_new + delivered_cur;
        if total > 0 {
            tracing::info!(delivered = total, "fastcore spool drain tick");
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// Walk one spool dir once, deliver every decodable file to its
/// recipient maildir(s), and remove it. Returns delivered count.
fn drain_once(dir: &Path, maildir_root: &str, state: &Arc<FastcoreState>) -> usize {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(dir = %dir.display(), error = %e, "spool dir read");
            }
            return 0;
        }
    };
    let mut delivered = 0;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
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
            } else {
                let via_alias = state.mailbox.resolve_alias(fwd).ok().flatten();
                via_alias.and_then(|a| {
                    if has_maildir(maildir_root, &a) {
                        tracing::info!(orig = %fwd, aliased = %a, "spool alias resolved");
                        Some(a)
                    } else {
                        None
                    }
                })
            };
            let Some(addr) = resolved_addr else {
                unresolved.push(fwd.clone());
                continue;
            };
            // 2. Consult the recipient's sieve script. Actions map to a
            //    Decision that overrides the default INBOX write.
            let decision = crate::sieve_apply::decide(&addr, body, Some(&env.reverse_path));
            match decision {
                crate::sieve_apply::Decision::Discard => {
                    delivered_here += 1;
                    tracing::info!(recipient = %addr, "sieve: discard");
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
                                    crate::ingest_delivered_file(state, &addr, &filename, body);
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
                            tracing::info!(recipient = %addr, %subfolder, "sieve: fileinto");
                            let blob_ref = format!("{subfolder}/{filename}");
                            crate::ingest_delivered_file(state, &addr, &blob_ref, body);
                        }
                        Ok(false) => {
                            tracing::warn!(recipient = %addr, %subfolder,
                                "sieve: fileinto target dir missing; falling back to INBOX");
                            if let Ok(true) = deliver(maildir_root, &addr, "", &filename, body) {
                                delivered_here += 1;
                                crate::ingest_delivered_file(state, &addr, &filename, body);
                            } else {
                                unresolved.push(addr.clone());
                            }
                        }
                        Err(e) => {
                            tracing::warn!(recipient = %addr, error = %e,
                                "sieve: fileinto write failed; falling back to INBOX");
                            if let Ok(true) = deliver(maildir_root, &addr, "", &filename, body) {
                                delivered_here += 1;
                                crate::ingest_delivered_file(state, &addr, &filename, body);
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
                            crate::ingest_delivered_file(state, &addr, &filename, body);
                        }
                        Ok(false) => unresolved.push(addr.clone()),
                        Err(e) => {
                            tracing::warn!(fwd = %addr, error = %e, "spool deliver");
                            unresolved.push(addr.clone());
                        }
                    }
                }
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
        } else {
            tracing::warn!(
                file = %filename,
                fwd_paths = ?env.forward_paths,
                "spool file has no resolvable recipient; leaving in place"
            );
        }
    }
    delivered
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
    let envelope = serde_json::json!({
        "sender": original_recipient,
        "recipients": [target],
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
}
