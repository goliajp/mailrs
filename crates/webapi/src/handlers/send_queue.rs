//! Putting an outgoing message on the queue, and having the core file the
//! sender's own copy.

use std::sync::Arc;

use crate::WebState;
use axum::http::StatusCode;

use crate::handlers::compose::*;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::prefs::now_secs;

/// Send mail from the system itself — no user compose, no Send row.
///
/// `noreply@{MAILRS_HOSTNAME}`, matching the monolith and the per-domain
/// DKIM override prod configures for exactly this sender.
///
/// `send_meta` is `None` on purpose: the Send view lists what a user sent,
/// and a password-reset notice is not that. Giving it a row would put mail
/// nobody composed into somebody's outbox.
pub(crate) fn send_system_mail(
    to: &str,
    subject: &str,
    text_body: &str,
    html_body: &str,
) -> Result<(), StatusCode> {
    let hostname = std::env::var("MAILRS_HOSTNAME").unwrap_or_default();
    if hostname.is_empty() {
        // No fallback. A guessed hostname produces a From nobody can reply
        // to and a link nobody can open, which is worse than a loud refusal
        // the operator can see in the logs.
        tracing::error!("MAILRS_HOSTNAME unset — refusing to send system mail");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let from = format!("noreply@{hostname}");
    let parts = ComposeParts {
        from: from.clone(),
        to: vec![to.to_string()],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: subject.to_string(),
        body: text_body.to_string(),
        html_body: html_body.to_string(),
        in_reply_to: None,
        forward_message_id: None,
        forward_attachments_from: None,
        reply_to_thread_id: None,
        attachments: Vec::new(),
        scheduled_at: None,
    };
    let (_message_id, envelope) = build_rfc5322(&parts, &from);
    enqueue_outbound_at(&from, &[to.to_string()], &envelope, None, None)
}

/// Enqueue an outbound message and record the send.
///
/// When `scheduled_at` is a future epoch, the id lands in the
/// `mailrs:outbound:scheduled` zset (scored by send time) instead of the
/// pending list; the sender's due-sweep promotes it when the time arrives.
///
/// `send_meta` carries what the Send view renders. It is written with
/// `?`, in this fallible step, on purpose: a send that returns 200 must
/// have its Send row, and a send whose row cannot be written must not be
/// sent.
pub(crate) fn enqueue_outbound_at(
    sender: &str,
    recipients: &[String],
    envelope: &[u8],
    scheduled_at: Option<i64>,
    send_meta: Option<SendMeta<'_>>,
) -> Result<(), StatusCode> {
    let created = now_secs();
    let send_at = scheduled_at.filter(|t| *t > created);
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(envelope);
    let sender = sender.to_string();
    // The job carries the group so the sender can report a per-recipient
    // outcome against the Send row (S2).
    let send_id_owned: Option<String> = send_meta.as_ref().map(|m| m.message_id.to_string());
    for rcpt in recipients {
        let rcpt = rcpt.trim().to_string();
        if rcpt.is_empty() {
            continue;
        }
        // Enqueue via the shared stone primitive so the write hits the
        // v2 job hash + pending-idx that sender actually reads. The
        // pre-2.9.38 form wrote a legacy `mailrs:outbound:{id}` +
        // `mailrs:outbound:pending`, which sender had stopped reading —
        // so every send from webapi silently stopped delivering until
        // an operator drained the queue by hand.
        let sender_c = sender.clone();
        let b64_c = b64.clone();
        let rcpt_c = rcpt.clone();
        let send_id_c = send_id_owned.clone();
        with_kevy(move |c| {
            mailrs_core_sidestate::families::outbound::write_fresh_pending(
                c,
                &mailrs_core_sidestate::families::outbound::FreshPending {
                    sender: &sender_c,
                    recipient: &rcpt_c,
                    message_data_base64: &b64_c,
                    scheduled_at: send_at,
                    original_sender: None,
                    send_id: send_id_c.as_deref(),
                },
                created,
            )
            .map(|_| ())
        })?;
    }

    if let Some(meta) = send_meta {
        use mailrs_core_sidestate::families::send as sendfam;
        let row = sendfam::SendRow {
            send_id: meta.message_id.to_string(),
            message_id: meta.message_id.to_string(),
            thread_id: meta.thread_id.to_string(),
            subject: meta.subject.to_string(),
            to_csv: meta.to.join(","),
            cc_csv: meta.cc.join(","),
            created_at: created,
            status: if send_at.is_some() {
                sendfam::Status::Scheduled
            } else {
                sendfam::Status::Sending
            },
            envelope_ref: meta.envelope_ref.to_string(),
            resent_from: meta.resent_from.map(str::to_string),
        };
        let user = sender.clone();
        let rcpts: Vec<String> = recipients.to_vec();
        with_kevy(move |c| sendfam::write_send(c, &user, &row, &rcpts))?;
    }
    Ok(())
}

/// Re-enqueue an already-composed envelope under a new send id.
///
/// Used by resend. The bytes go out unchanged — same Message-ID, same
/// attachments — because a resend after failure is the same message to
/// someone who never got it. `envelope_ref` carries over so the new row
/// can itself be resent or re-edited without re-reading the old one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn enqueue_resend(
    user: &str,
    recipients: &[String],
    envelope: &[u8],
    send_id: &str,
    thread_id: &str,
    subject: &str,
    resent_from: &str,
    envelope_ref: &str,
) -> Result<(), StatusCode> {
    let to: Vec<String> = recipients.to_vec();
    let empty: Vec<String> = Vec::new();
    enqueue_outbound_at(
        user,
        recipients,
        envelope,
        None,
        Some(SendMeta {
            message_id: send_id,
            thread_id,
            subject,
            to: &to,
            cc: &empty,
            // The original's maildir file holds these same bytes, so the
            // resend points at it rather than writing a second copy.
            //
            // It used to say that and pass `""`, which points at
            // nothing: `can_resend()` reads an empty ref as "the maildir
            // write failed and the bytes are not on disk", so every
            // resent message showed "the stored copy of this message is
            // missing, so it cannot be resent or edited" — while the
            // original's file sat right there. Found 2026-08-17 on a
            // resend that Microsoft rejected; the user could not try
            // again.
            envelope_ref,
            resent_from: Some(resent_from),
        }),
    )
}

/// File the sender's own copy of `envelope` with the core that owns their
/// mailbox: the maildir file in Sent, the index, usage and contacts are
/// all its writes. `recipients` is the whole envelope, Bcc included,
/// since the bytes do not name the Bcc recipients.
pub(crate) async fn file_sent_copy(
    state: &Arc<WebState>,
    user: &str,
    envelope: &[u8],
    recipients: &[String],
) -> Result<mailrs_core_api::method::sent::FileSentResponse, StatusCode> {
    state
        .core
        .file_sent(user, envelope, recipients)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, %user, "the core did not file the sent copy");
            crate::handlers::conversations::map_err(e)
        })
}
