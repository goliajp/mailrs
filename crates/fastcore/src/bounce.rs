//! Bounce DSN generation + hand-off queue (G9, RFC 3464).
//!
//! The sender process detects permanent failures but owns no maildir;
//! fastcore owns delivery. Hand-off is a network-kevy queue:
//!
//!   LPUSH mailrs:bounce:pending <id>
//!   HSET  mailrs:bounce:<id> recipient=<local sender> blob=<b64 DSN>
//!
//! fastcore's bounce drain (`spawn_bounce_drain`) pops ids, writes the
//! DSN into the recipient's maildir and runs the normal ingest
//! write-through — threading, uid, realtime push all come along.
//!
//! Double-bounce protection: no DSN is generated when the failed
//! message's envelope sender is null (`<>`), a MAILER-DAEMON, or a
//! postmaster address; the DSN itself is composed so a bounce OF the
//! bounce would hit exactly that guard on the remote side too.

use std::sync::Arc;

use base64::Engine as _;

use crate::FastcoreState;

/// Pending-queue key in the network kevy.
pub const BOUNCE_PENDING: &[u8] = b"mailrs:bounce:pending";

/// True when we must NOT generate a DSN for a failure of mail from
/// this envelope sender (double-bounce guard).
pub fn suppress_bounce(envelope_sender: &str) -> bool {
    let s = envelope_sender
        .trim()
        .trim_matches(|c| c == '<' || c == '>');
    if s.is_empty() {
        return true;
    }
    let local = s.split('@').next().unwrap_or("").to_ascii_lowercase();
    local == "mailer-daemon" || local == "postmaster"
}

/// Pull `Message-ID` and `References` header values (raw, unfolded)
/// out of the original message head for DSN threading.
fn threading_headers(original: &[u8]) -> (Option<String>, Option<String>) {
    let head = &original[..original.len().min(16 * 1024)];
    let (mid, in_reply_to, refs_first, ..) = crate::extract_headers(head);
    let _ = in_reply_to;
    let mid = (!mid.is_empty()).then_some(mid);
    let refs = (!refs_first.is_empty()).then_some(refs_first);
    (mid, refs)
}

/// Original header block (up to the first blank line, capped at 8 KB)
/// for the text/rfc822-headers part.
fn original_headers(original: &[u8]) -> Vec<u8> {
    let cap = original.len().min(8 * 1024);
    let slice = &original[..cap];
    let end = slice
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 2)
        .or_else(|| slice.windows(2).position(|w| w == b"\n\n").map(|p| p + 1))
        .unwrap_or(cap);
    slice[..end].to_vec()
}

/// Compose an RFC 3464 multipart/report DSN.
///
/// `reporting_mta` — our HELO hostname; `original_sender` — the local
/// user who sent the failed message (DSN recipient); `failed_recipient`
/// — the remote address that failed; `diagnostic` — remote SMTP reply
/// or local reason.
pub fn compose_dsn(
    reporting_mta: &str,
    original_sender: &str,
    failed_recipient: &str,
    diagnostic: &str,
    original: &[u8],
) -> Vec<u8> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let boundary = format!("=_mailrs_dsn_{now}");
    let dsn_mid = format!("<dsn-{now}-{}@{reporting_mta}>", now % 997);
    let (orig_mid, orig_refs) = threading_headers(original);
    let date = chrono::DateTime::from_timestamp(now as i64, 0)
        .map(|d| d.to_rfc2822())
        .unwrap_or_default();

    let mut h = String::new();
    h.push_str(&format!(
        "From: Mail Delivery System <MAILER-DAEMON@{reporting_mta}>\r\n"
    ));
    h.push_str(&format!("To: <{original_sender}>\r\n"));
    h.push_str("Subject: Undelivered Mail Returned to Sender\r\n");
    h.push_str(&format!("Date: {date}\r\n"));
    h.push_str(&format!("Message-ID: {dsn_mid}\r\n"));
    if let Some(mid) = &orig_mid {
        h.push_str(&format!("In-Reply-To: <{mid}>\r\n"));
        // thread into the ORIGINAL conversation: root reference first
        match &orig_refs {
            Some(root) if root != mid => {
                h.push_str(&format!("References: <{root}> <{mid}>\r\n"));
            }
            _ => h.push_str(&format!("References: <{mid}>\r\n")),
        }
    }
    h.push_str("Auto-Submitted: auto-replied\r\n");
    h.push_str("MIME-Version: 1.0\r\n");
    h.push_str(&format!(
        "Content-Type: multipart/report; report-type=delivery-status; boundary=\"{boundary}\"\r\n"
    ));
    h.push_str("\r\n");

    let mut b = h.into_bytes();
    let human = format!(
        "--{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n\
         This is the mail system at {reporting_mta}.\r\n\r\n\
         Your message could not be delivered to the following recipient:\r\n\r\n\
             {failed_recipient}\r\n\r\n\
         Remote server said:\r\n    {diagnostic}\r\n\r\n\
         The message was not delivered and will not be retried.\r\n\r\n"
    );
    b.extend_from_slice(human.as_bytes());
    let status = format!(
        "--{boundary}\r\nContent-Type: message/delivery-status\r\n\r\n\
         Reporting-MTA: dns; {reporting_mta}\r\n\r\n\
         Final-Recipient: rfc822; {failed_recipient}\r\n\
         Action: failed\r\nStatus: 5.0.0\r\n\
         Diagnostic-Code: smtp; {diag}\r\n\r\n",
        diag = diagnostic.replace(['\r', '\n'], " ")
    );
    b.extend_from_slice(status.as_bytes());
    b.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: text/rfc822-headers\r\n\r\n").as_bytes(),
    );
    b.extend_from_slice(&original_headers(original));
    b.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    b
}

/// Drain the bounce hand-off queue: deliver each DSN into the local
/// recipient's maildir and run the standard ingest write-through.
/// Unknown recipients (not a kevy account, no maildir) are dropped
/// with a warn — a bounce must never itself bounce.
pub fn spawn_bounce_drain(state: Arc<FastcoreState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Some(url) = crate::live_sync::network_kevy_url() else {
            tracing::info!("no network kevy — bounce drain disabled");
            return;
        };
        let maildir_root =
            std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        loop {
            drain_once(&state, &url, &maildir_root);
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    })
}

fn drain_once(state: &Arc<FastcoreState>, url: &str, maildir_root: &str) {
    let Ok(mut conn) = kevy_client::Connection::open(url) else {
        return;
    };
    loop {
        let popped = conn.rpop(BOUNCE_PENDING, 1).unwrap_or_default();
        let Some(id_bytes) = popped.into_iter().next() else {
            return;
        };
        let Ok(id) = String::from_utf8(id_bytes) else {
            continue;
        };
        let key = format!("mailrs:bounce:{id}");
        let recipient = conn
            .hget(key.as_bytes(), b"recipient")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let blob = conn
            .hget(key.as_bytes(), b"blob")
            .ok()
            .flatten()
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok());
        let _ = conn.del(&[key.as_bytes()]);
        let Some(bytes) = blob else { continue };
        // local-account guard: never forward a DSN outward
        let known = state
            .mailbox
            .get_account_blob(&recipient)
            .ok()
            .flatten()
            .is_some();
        let Some((local, domain)) = recipient.split_once('@') else {
            continue;
        };
        let base = std::path::PathBuf::from(maildir_root)
            .join(domain)
            .join(local);
        if !known && !base.join("new").is_dir() {
            tracing::warn!(%recipient, "bounce for unknown local sender dropped");
            continue;
        }
        for leaf in ["cur", "new", "tmp"] {
            let _ = std::fs::create_dir_all(base.join(leaf));
        }
        let filename = format!(
            "{}.Mdsn{}.bounce",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            id
        );
        let target = base.join("new").join(&filename);
        if let Err(e) = std::fs::write(&target, &bytes) {
            tracing::warn!(error = %e, %recipient, "bounce maildir write failed");
            continue;
        }
        crate::ingest_delivered_file(state, &recipient, &filename, &bytes);
        tracing::info!(%recipient, %id, "bounce DSN delivered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppress_null_and_daemon_senders() {
        assert!(suppress_bounce(""));
        assert!(suppress_bounce("<>"));
        assert!(suppress_bounce("MAILER-DAEMON@x.y"));
        assert!(suppress_bounce("postmaster@x.y"));
        assert!(!suppress_bounce("user@x.y"));
    }

    #[test]
    fn dsn_threads_into_original_conversation() {
        let orig = b"Message-ID: <orig@x.y>\r\nReferences: <root@x.y>\r\nSubject: hi\r\n\r\nbody";
        let dsn = compose_dsn(
            "mx.test",
            "sender@x.y",
            "gone@remote.z",
            "550 no such user",
            orig,
        );
        let text = String::from_utf8_lossy(&dsn);
        assert!(text.contains("References: <root@x.y> <orig@x.y>"));
        assert!(text.contains("In-Reply-To: <orig@x.y>"));
        assert!(text.contains("Final-Recipient: rfc822; gone@remote.z"));
        assert!(text.contains("Action: failed"));
        assert!(text.contains("Auto-Submitted: auto-replied"));
        assert!(text.contains("To: <sender@x.y>"));
        // original headers echoed in the report
        assert!(text.contains("Subject: hi"));
    }

    #[test]
    fn dsn_without_original_mid_still_valid() {
        let dsn = compose_dsn("mx.test", "s@x.y", "r@z.w", "timeout", b"no headers here");
        let text = String::from_utf8_lossy(&dsn);
        assert!(!text.contains("In-Reply-To"));
        assert!(text.contains("multipart/report"));
    }
}
