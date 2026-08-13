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
    let (mid, in_reply_to, references, ..) = crate::extract_headers(head);
    let _ = in_reply_to;
    let mid = (!mid.is_empty()).then_some(mid);
    let refs = references.first().cloned();
    (mid, refs)
}

/// Case-insensitive unfolded single-header lookup over a raw message
/// head. Shared by DSN composition and the vacation suppression rules.
pub(crate) fn header_value(raw: &[u8], name: &str) -> Option<String> {
    let head = &raw[..raw.len().min(16 * 1024)];
    let text = String::from_utf8_lossy(head);
    let head_end = text.find("\r\n\r\n").or_else(|| text.find("\n\n"));
    let head = &text[..head_end.unwrap_or(text.len())];
    let want = name.to_ascii_lowercase();
    let mut current: Option<String> = None;
    for line in head.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(c) = &mut current {
                c.push(' ');
                c.push_str(line.trim_start());
            }
            continue;
        }
        if current.is_some() {
            break; // finished collecting the wanted header
        }
        if let Some((n, v)) = line.split_once(':')
            && n.trim().to_ascii_lowercase() == want
        {
            current = Some(v.trim().to_string());
        }
    }
    current
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

/// The two identities a DSN needs, read from the environment.
///
/// Returns `(reporting_mta, from_domain)`. They are deliberately
/// separate values — see [`compose_dsn`] for why conflating them
/// breaks either RFC 3464 or DMARC.
///
/// - `reporting_mta` ← `MAILRS_HELO_HOSTNAME`
/// - `from_domain` ← `MAILRS_DSN_FROM_DOMAIN`, falling back to
///   `reporting_mta` so an unset variable preserves the old behaviour.
pub fn dsn_identity() -> (String, String) {
    dsn_identity_from(
        std::env::var("MAILRS_HELO_HOSTNAME").ok().as_deref(),
        std::env::var("MAILRS_DSN_FROM_DOMAIN").ok().as_deref(),
    )
}

/// The decision itself, with the environment already read.
///
/// Split out because the tests for it used to `set_var` on process-global
/// state, and cargo runs a binary's tests in parallel threads — two tests
/// racing the same two variables saw each other's values, which is how
/// `dsn_identity_fallback_never_yields_a_subdomain` failed inside a full
/// workspace run and passed on its own. A mutex would have serialised the
/// race; taking the inputs as arguments removes it, and the logic is
/// worth testing without an environment anyway.
pub fn dsn_identity_from(helo: Option<&str>, dsn_from_domain: Option<&str>) -> (String, String) {
    let reporting_mta = helo
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_REPORTING_MTA)
        .to_string();
    let from_domain = match dsn_from_domain.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => d.to_string(),
        None => organisational_domain(&reporting_mta),
    };
    (reporting_mta, from_domain)
}

/// Reduce a hostname to its registrable domain — `mail.golia.ai` →
/// `golia.ai`. Returns the input unchanged when the public-suffix list
/// cannot parse it (a bare hostname, an address literal).
///
/// The fallback path uses this instead of the MTA hostname directly,
/// and the distinction is not cosmetic. A DSN `From:` on a *subdomain*
/// is evaluated against the organisational domain's `sp=` rather than
/// its `p=`, and needs a DKIM key published for that exact subdomain to
/// align. Neither is usually true, so falling back to the hostname
/// produces bounces that fail DMARC under a stricter policy than the
/// domain's own mail.
///
/// This was a live defect: v2.10.0 set MAILRS_HELO_HOSTNAME to a real
/// FQDN while one of the two containers was missing
/// MAILRS_DSN_FROM_DOMAIN, so its bounces went out as
/// MAILER-DAEMON@mail.golia.ai — squarely under `sp=reject`. The
/// earlier `mailrs` default had been accidentally safe, because a
/// nonexistent domain has no policy to be rejected by. Config is still
/// the right place to set this; the fallback just no longer picks a
/// value that is worse than picking nothing.
fn organisational_domain(host: &str) -> String {
    let host = host.trim().trim_end_matches('.');
    match psl::domain_str(host) {
        Some(d) => d.to_string(),
        None => host.to_string(),
    }
}

/// Fallback when `MAILRS_HELO_HOSTNAME` is unset.
///
/// Not a valid FQDN, and deliberately so: it is obviously wrong in
/// logs and headers rather than quietly plausible. Production must set
/// the variable.
const DEFAULT_REPORTING_MTA: &str = "mailrs";

/// Compose an RFC 3464 multipart/report DSN.
///
/// Two distinct identities, because their requirements conflict:
///
/// - `reporting_mta` — the **MTA's hostname**. RFC 3464 §2.2.2 wants a
///   DNS name for the `Reporting-MTA:` field, so this must be the host
///   that actually handled the message (e.g. `mail.golia.ai`), matching
///   the sending IP's PTR.
/// - `from_domain` — the domain in `MAILER-DAEMON@…`. This one is
///   subject to DMARC at the receiving end, so it must be a domain we
///   can align a DKIM signature to. Using the MTA hostname here puts a
///   *subdomain* in `From:`, which falls under `sp=` rather than `p=`
///   and needs a signing key published for that exact subdomain.
///
/// Postfix draws the same line (`Reporting-MTA` from `myhostname`,
/// the envelope from `myorigin`). Passing one value for both is what
/// produced `MAILER-DAEMON@mailrs` in production.
///
/// `original_sender` — the local user who sent the failed message (DSN
/// recipient); `failed_recipient` — the remote address that failed;
/// `diagnostic` — remote SMTP reply or local reason.
pub fn compose_dsn(
    reporting_mta: &str,
    from_domain: &str,
    original_sender: &str,
    failed_recipient: &str,
    status: &str,
    diagnostic: &str,
    original: &[u8],
) -> Vec<u8> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let boundary = format!("=_mailrs_dsn_{now}");
    // Message-ID shares the From: domain — a Message-ID whose domain
    // does not resolve is another reputation signal.
    let dsn_mid = format!("<dsn-{now}-{}@{from_domain}>", now % 997);
    let (orig_mid, orig_refs) = threading_headers(original);
    let date = chrono::DateTime::from_timestamp(now as i64, 0)
        .map(|d| d.to_rfc2822())
        .unwrap_or_default();

    let mut h = String::new();
    h.push_str(&format!(
        "From: Mail Delivery System <MAILER-DAEMON@{from_domain}>\r\n"
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
         Action: failed\r\nStatus: {status}\r\n\
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
///
/// v2.2 §3 (2026-07-08): the outer 10 s polling sleep is gone —
/// `drain_one` blocks on `brpop(BOUNCE_PENDING, Some(10s))`, so a
/// newly-enqueued bounce is processed synchronously with the
/// producer's `LPUSH`; the timer only fires when the queue has been
/// empty for the whole 10 s window, at which point the task loops
/// straight back into another blocking `BRPOP`. No wall-clock
/// budget spent on polling an empty queue.
///
/// The loop runs on `spawn_blocking`, not on a runtime worker.
/// `drain_one` is synchronous and blocks for up to 10 s with no `.await`
/// anywhere in the loop, so a plain `tokio::spawn` pinned one worker
/// thread for the life of the process and never gave it back — a quarter
/// of the async capacity on a four-core host. It also made shutdown
/// impossible: a worker parked in a blocking syscall with no yield point
/// never observes the runtime's shutdown, so the process flushed kevy on
/// SIGTERM, logged that it had, and then sat there. Measured before this
/// change: 0.55 s to exit with no `MAILRS_KEVY_URL`, still alive after
/// 40 s with one set — which is the production configuration, so every
/// deploy was waiting out `docker stop`'s grace period and being killed.
///
/// `kevy/no-blocking-pop-wrap` already required this, and already listed
/// this call site as one of its compliant callers.
pub fn spawn_bounce_drain(state: Arc<FastcoreState>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let Some(url) = crate::live_sync::network_kevy_url() else {
            tracing::info!("no network kevy — bounce drain disabled");
            return;
        };
        let maildir_root =
            std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        loop {
            drain_one(&state, &url, &maildir_root);
        }
    })
}

fn drain_one(state: &Arc<FastcoreState>, url: &str, maildir_root: &str) {
    let Ok(mut conn) = kevy_client::Connection::connect(url) else {
        return;
    };
    // First pop blocks up to 10 s (matches prior wake-up cadence); after
    // that first hit we drain remaining items with non-blocking RPOPs to
    // avoid re-arming the blocking timer per item on a bursty queue.
    let mut first = true;
    loop {
        let popped_bytes: Option<Vec<u8>> = if first {
            first = false;
            match conn.brpop(&[BOUNCE_PENDING], Some(std::time::Duration::from_secs(10))) {
                Ok(Some((_key, value))) => Some(value),
                Ok(None) | Err(_) => None,
            }
        } else {
            conn.rpop(BOUNCE_PENDING, 1)
                .unwrap_or_default()
                .into_iter()
                .next()
        };
        let Some(id_bytes) = popped_bytes else {
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
        // Bounce DSNs are inbound-side notifications to the sender —
        // always route to INBOX, never Junk (v2.4.0 Phase 2).
        crate::ingest_delivered_file(state, &recipient, &filename, &bytes, "INBOX");
        crate::live_sync::audit_system("mail.bounce", &recipient, "DSN delivered to sender");
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
            "test.example",
            "sender@x.y",
            "gone@remote.z",
            "5.1.1",
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
    fn dsn_separates_reporting_mta_from_the_sender_domain() {
        // The MTA hostname belongs in Reporting-MTA (RFC 3464 §2.2.2);
        // the From: domain has to be DMARC-alignable. Conflating them
        // is what shipped MAILER-DAEMON@mailrs to production.
        let dsn = compose_dsn(
            "mail.golia.ai",
            "golia.ai",
            "sender@golia.ai",
            "gone@remote.z",
            "5.1.1",
            "550 no such user",
            b"Subject: hi\r\n\r\nbody",
        );
        let text = String::from_utf8_lossy(&dsn);

        assert!(
            text.contains("From: Mail Delivery System <MAILER-DAEMON@golia.ai>"),
            "From: must use the alignable domain, not the MTA hostname"
        );
        assert!(
            text.contains("Reporting-MTA: dns; mail.golia.ai"),
            "Reporting-MTA must be the MTA hostname"
        );
        assert!(text.contains("This is the mail system at mail.golia.ai."));
        assert!(
            text.contains("@golia.ai>\r\n"),
            "Message-ID must share the From: domain so it resolves"
        );
    }

    #[test]
    fn dsn_identity_prefers_explicit_config() {
        unsafe {
            std::env::set_var("MAILRS_HELO_HOSTNAME", "mail.golia.ai");
            std::env::set_var("MAILRS_DSN_FROM_DOMAIN", "golia.ai");
        }
        let (mta, from) = dsn_identity();
        assert_eq!(mta, "mail.golia.ai");
        assert_eq!(from, "golia.ai");

        unsafe {
            std::env::remove_var("MAILRS_HELO_HOSTNAME");
            std::env::remove_var("MAILRS_DSN_FROM_DOMAIN");
        }
    }

    #[test]
    fn dsn_identity_fallback_never_yields_a_subdomain() {
        // The regression this guards: a deployment that sets a real
        // FQDN for HELO but forgets MAILRS_DSN_FROM_DOMAIN used to send
        // bounces From: a subdomain, which is judged by sp= and has no
        // aligned key. Falling back to the organisational domain keeps
        // the bounce under the same policy as the domain's own mail.
        let (mta, from) = dsn_identity_from(Some("mail.golia.ai"), None);

        assert_eq!(mta, "mail.golia.ai", "Reporting-MTA stays the host");
        assert_eq!(from, "golia.ai", "From: drops to the organisational domain");
    }

    #[test]
    fn an_explicit_dsn_domain_wins_and_blank_counts_as_unset() {
        assert_eq!(
            dsn_identity_from(Some("mail.golia.ai"), Some("golia.jp")),
            ("mail.golia.ai".into(), "golia.jp".into())
        );
        // A variable present but empty is a deployment that meant to set
        // it and did not; treating it as set would put the bounce on an
        // empty domain.
        assert_eq!(
            dsn_identity_from(Some("mail.golia.ai"), Some("   ")),
            ("mail.golia.ai".into(), "golia.ai".into())
        );
    }

    #[test]
    fn organisational_domain_reduces_subdomains() {
        assert_eq!(organisational_domain("mail.golia.ai"), "golia.ai");
        assert_eq!(organisational_domain("a.b.c.golia.jp"), "golia.jp");
        assert_eq!(organisational_domain("mail.golia.ai."), "golia.ai");
    }

    #[test]
    fn organisational_domain_leaves_a_registrable_domain_alone() {
        assert_eq!(organisational_domain("golia.ai"), "golia.ai");
        assert_eq!(organisational_domain("dadaya.jp"), "dadaya.jp");
    }

    #[test]
    fn organisational_domain_passes_through_what_it_cannot_parse() {
        // The bare-hostname default. Not a valid From: domain either
        // way, but it must not panic or silently become something else.
        assert_eq!(organisational_domain("mailrs"), "mailrs");
        assert_eq!(organisational_domain(""), "");
    }

    #[test]
    fn dsn_without_original_mid_still_valid() {
        let dsn = compose_dsn(
            "mx.test",
            "test.example",
            "s@x.y",
            "r@z.w",
            "5.0.0",
            "timeout",
            b"no headers here",
        );
        let text = String::from_utf8_lossy(&dsn);
        assert!(!text.contains("In-Reply-To"));
        assert!(text.contains("multipart/report"));
    }
}
