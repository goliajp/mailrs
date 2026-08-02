//! What happens to one spooled envelope: alias resolution, delivery,
//! and the two outbound enqueues.

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

use std::path::PathBuf;
use std::sync::Arc;

use crate::FastcoreState;

/// Deliver one file to one recipient. `subfolder` is empty for INBOX
/// or `.Maildir++Folder` (produced by sieve_apply::maildir_subfolder)
/// for a fileinto action. Returns `Ok(true)` on success, `Ok(false)`
/// when the target directory is absent.
pub(crate) fn deliver(
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
pub(crate) fn provision_if_account(
    state: &Arc<FastcoreState>,
    maildir_root: &str,
    addr: &str,
) -> bool {
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
pub(crate) fn has_maildir(maildir_root: &str, addr: &str) -> bool {
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
pub(crate) fn enqueue_outbound(to: &str, sender: &str, body: &[u8]) -> std::io::Result<()> {
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
pub(crate) fn enqueue_redirect(
    original_recipient: &str,
    target: &str,
    body: &[u8],
    reverse_path: &str,
) -> std::io::Result<()> {
    use base64::Engine as _;
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        return Err(std::io::Error::other("MAILRS_KEVY_URL unset"));
    };
    let mut conn = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
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
    // Enqueue through the shared primitive, never by hand. It writes
    // `mailrs:outbound:job:{id}` with state=pending plus the
    // `pending-idx` list, which is what the sender's BRPOP + WATCH/CAS
    // claim actually reads.
    //
    // This used to hand-roll `mailrs:outbound:{id}` + the legacy
    // `pending` list. Nothing consumes those in the fastcore topology,
    // so every DSN, every sieve redirect and every SRS-reversed bounce
    // was written to a queue with no reader and silently never sent.
    // webapi had the identical bug and was fixed in 2.9.38; these
    // callers were missed. See write_fresh_pending's own doc comment.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    mailrs_core_sidestate::families::outbound::write_fresh_pending(
        &mut conn,
        &mailrs_core_sidestate::families::outbound::FreshPending {
            sender: &mail_from,
            recipient: target,
            message_data_base64: &b64_body,
            scheduled_at: None,
            original_sender: Some(reverse_path),
            // A sieve redirect forwards mail the user did not write.
            send_id: None,
        },
        now,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::spool_drain::*;
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

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
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

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
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

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
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

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &state());
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
            drain_once(&missing, maildir_root.to_str().unwrap(), &state()).0,
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

        let (delivered, _) = drain_once(&spool_new, maildir_root.to_str().unwrap(), &s);
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
