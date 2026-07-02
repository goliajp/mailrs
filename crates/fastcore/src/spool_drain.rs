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
            // First try the direct mailbox. If missing, fall through to
            // the alias table (`mailrs:alias:<addr>` → target). Prevents
            // a `contact@golia.jp → lihao@golia.jp` alias from silently
            // dead-lettering when nobody sits at contact/.
            let target = match deliver(maildir_root, fwd, &filename, body) {
                Ok(true) => {
                    delivered_here += 1;
                    None
                }
                Ok(false) => Some(fwd.clone()),
                Err(e) => {
                    tracing::warn!(fwd = %fwd, error = %e, "spool deliver");
                    Some(fwd.clone())
                }
            };
            if let Some(orig) = target {
                let resolved = state
                    .mailbox
                    .resolve_alias(&orig)
                    .ok()
                    .flatten();
                match resolved.as_deref() {
                    Some(aliased) => match deliver(maildir_root, aliased, &filename, body) {
                        Ok(true) => {
                            delivered_here += 1;
                            tracing::info!(
                                orig = %orig, aliased = %aliased,
                                "spool alias resolved"
                            );
                        }
                        Ok(false) => unresolved.push(format!("{orig} (alias→{aliased})")),
                        Err(e) => {
                            tracing::warn!(fwd = %aliased, error = %e, "spool deliver via alias");
                            unresolved.push(format!("{orig} (alias→{aliased})"));
                        }
                    },
                    None => unresolved.push(orig),
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

/// Deliver one file to one recipient. Returns `Ok(true)` on success,
/// `Ok(false)` when the recipient has no maildir (unresolved).
fn deliver(maildir_root: &str, addr: &str, filename: &str, body: &[u8]) -> std::io::Result<bool> {
    let (local, domain) = match addr.split_once('@') {
        Some(x) => x,
        None => return Ok(false),
    };
    let user_new = PathBuf::from(maildir_root)
        .join(domain)
        .join(local)
        .join("new");
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

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
    use mailrs_mailbox_kevy::KevyMailboxStore;

    fn state() -> Arc<FastcoreState> {
        let store = Arc::new(Store::open(Config::default()).unwrap());
        let mailbox = KevyMailboxStore::new(store);
        Arc::new(FastcoreState { mailbox })
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
            alice_base
                .join("new")
                .join("1000003.M1P1Q1.host")
                .exists(),
            "aliased delivery must land in the resolved user's maildir"
        );
    }
}
