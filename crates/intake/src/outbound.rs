//! Mail the drain sends on rather than delivers: forwards, sieve redirects,
//! DSNs and authenticated submissions to other domains. All of it goes on
//! the shared outbound queue `mailrs-fastcore-sender` claims from.

use base64::Engine as _;
use mailrs_core_sidestate::families::outbound::{
    FreshPending, write_fresh_pending, write_fresh_pending_once,
};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn connect() -> std::io::Result<kevy_client::Connection> {
    let url = std::env::var("MAILRS_KEVY_URL")
        .map_err(|_| std::io::Error::other("MAILRS_KEVY_URL unset"))?;
    Ok(kevy_client::Connection::connect(&url)?)
}

/// Enqueue an arbitrary outbound message (DSN / auto-reply). `sender`
/// is the MAIL FROM ("<>" for null-envelope notifications). With `once`,
/// queued once for that key ([`write_fresh_pending_once`]).
pub fn enqueue_outbound(
    to: &str,
    sender: &str,
    body: &[u8],
    once: Option<&str>,
) -> std::io::Result<()> {
    let mail_from = if sender.is_empty() { "<>" } else { sender };
    enqueue_relay(mail_from, to, body, mail_from, once)
}

/// The job written, once for `once` when it is given.
fn write_job(
    conn: &mut kevy_client::Connection,
    job: &FreshPending<'_>,
    once: Option<&str>,
) -> std::io::Result<Option<i64>> {
    match once {
        Some(key) => {
            let written = write_fresh_pending_once(conn, job, key, now())?;
            if written.is_none() {
                tracing::info!(to = %job.recipient, "outbound job already queued by an earlier attempt");
            }
            Ok(written)
        }
        None => write_fresh_pending(conn, job, now()).map(Some),
    }
}

/// A security alert from `from` to `to`: neither counted toward nor held
/// by an outbound pause, whatever `from`'s state. Only alert senders call
/// this.
pub fn enqueue_security_alert(to: &str, from: &str, body: &[u8]) -> std::io::Result<()> {
    let mut conn = connect()?;
    let b64_body = base64::engine::general_purpose::STANDARD.encode(body);
    mailrs_core_sidestate::families::outbound::write_security_alert_pending(
        &mut conn,
        &FreshPending {
            sender: from,
            recipient: to,
            message_data_base64: &b64_body,
            scheduled_at: None,
            original_sender: None,
            send_id: None,
            account: None,
        },
        now(),
    )?;
    Ok(())
}

/// Forward `body`, addressed to `original_recipient` here, on to `target`.
///
/// The copy carries a `Delivered-To:` line naming `original_recipient`, as
/// Postfix writes one per hop. Two addresses forwarding to each other, here
/// or through another server, send the message back with that line in it,
/// and it is not forwarded again: the error names the loop, and every
/// caller keeps the copy here or reports the address as not reached.
pub fn enqueue_redirect(
    original_recipient: &str,
    target: &str,
    body: &[u8],
    reverse_path: &str,
    once: Option<&str>,
) -> std::io::Result<()> {
    if let Some(why) = crate::forward_loop::refused(body, original_recipient) {
        return Err(std::io::Error::other(format!("not forwarded: {why}")));
    }
    let marked = crate::forward_loop::marked(body, original_recipient);
    enqueue_relay(original_recipient, target, &marked, reverse_path, once)
}

fn enqueue_relay(
    original_recipient: &str,
    target: &str,
    body: &[u8],
    reverse_path: &str,
    once: Option<&str>,
) -> std::io::Result<()> {
    let mut conn = connect()?;
    let b64_body = base64::engine::general_purpose::STANDARD.encode(body);
    // When we forward on behalf of an external sender, the receiving MX runs
    // SPF against our IP, so a foreign reverse-path is rewritten with SRS onto
    // our domain; it reverses so bounces reach the original sender. Null
    // senders and senders on our domain stay as they are.
    let mail_from = match crate::route::srs_secret() {
        Some(secret) if !reverse_path.trim().trim_matches(['<', '>']).is_empty() => {
            let our_domain = original_recipient.split('@').nth(1).unwrap_or("");
            let rp = reverse_path.trim().trim_matches(['<', '>']);
            if rp.split('@').nth(1) == Some(our_domain) {
                rp.to_string()
            } else {
                mailrs_srs::rewrite(rp, our_domain, &secret)
            }
        }
        _ => original_recipient.to_string(),
    };
    // Through the shared primitive, never by hand: it writes the job row and
    // the pending index the sender's claim reads.
    write_job(
        &mut conn,
        &FreshPending {
            sender: &mail_from,
            recipient: target,
            message_data_base64: &b64_body,
            scheduled_at: None,
            original_sender: Some(reverse_path),
            // a forward carries mail the user did not write
            send_id: None,
            account: None,
        },
        once,
    )?;
    Ok(())
}

/// One outbound job for `rcpt`. The envelope sender is the submitting
/// user's own address, so there is no SRS rewrite as there is for a
/// forward.
pub fn enqueue_submission(
    sender: &str,
    account: Option<&str>,
    rcpt: &str,
    body: &[u8],
    once: Option<&str>,
) -> std::io::Result<()> {
    let mut conn = connect()?;
    let b64_body = base64::engine::general_purpose::STANDARD.encode(body);
    let sender = sender.trim().trim_matches(['<', '>']);
    let id = write_job(
        &mut conn,
        &FreshPending {
            sender,
            recipient: rcpt,
            message_data_base64: &b64_body,
            scheduled_at: None,
            original_sender: None,
            // composed in a mail client, not in the web UI's Send list
            send_id: None,
            account,
        },
        once,
    )?;
    if let Some(id) = id {
        tracing::info!(from = %sender, to = %rcpt, job = id, "submission queued for relay");
    }
    Ok(())
}
