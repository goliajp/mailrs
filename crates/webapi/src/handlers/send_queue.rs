//! Putting an outgoing message on the queue, and mirroring it into the
//! sender's own view.
//!
//! `enqueue_outbound_at` is the fallible half the send depends on;
//! `mirror_send_to_sender_view` is best-effort throughout and returns
//! `()`. `.claude/rfcs/20260730-send-status.md` is about that asymmetry.

use std::sync::Arc;

use crate::WebState;
use axum::http::StatusCode;

use crate::handlers::compose::*;
use crate::handlers::prefs::{now_secs, with_kevy};

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

/// Enqueue outbound. When `scheduled_at` is a future epoch, the id
/// lands in the `mailrs:outbound:scheduled` zset (scored by send time)
/// instead of the pending list; the sender's due-sweep promotes it to
/// pending when the time arrives (G13). Past / None sends immediately.
/// Enqueue an outbound message and record the send.
///
/// `send_meta` carries what the Send view renders. It is written with
/// `?`, in this fallible step, on purpose: a send that returns 200 must
/// have its Send row, and a send whose row cannot be written must not be
/// sent. The alternative is what this replaces —
/// `mirror_send_to_sender_view`, which records the same fact
/// best-effort, returns `()`, and left a delivered mail invisible in
/// Sent for 1m42s on 2026-07-30 (RFC 20260730-send-status).
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
            envelope_ref: "",
            resent_from: Some(resent_from),
        }),
    )
}

/// Mirror an outbound send / draft save into the sender's own kevy
/// view so it shows up in the Sent (or Drafts) tab immediately, into
/// their maildir so IMAP sees it, and into the contacts hash so
/// recipient autocomplete stays fresh.
///
/// `is_draft = true` marks the wire with `FLAG_DRAFT` and lands the
/// message under a Draft-flavored kevy thread; `false` marks
/// `FLAG_SEEN` (the sender always "already read" what they wrote).
///
/// This intentionally does one write per persistence layer and
/// swallows individual failures with a warning instead of failing the
/// whole request — the primary user-facing operation is the send
/// itself (kevy outbound queue), and the mirror is a UX nicety that
/// mustn't take the send down with it.
pub(crate) async fn mirror_send_to_sender_view(
    state: &Arc<WebState>,
    user: &str,
    parts: &ComposeParts,
    envelope: &[u8],
    message_id: &str,
    is_draft: bool,
) {
    use mailrs_core_api::method::message::MessageWire;
    use mailrs_core_api::method::thread::DeliverMessageRequest;
    use mailrs_message_store::{MaildirStore, MessageStore};

    let now = now_secs();
    // v2.9.5 threading fix — a reply must join the thread its parent
    // actually lives in (msgid → thread index via core-api), NOT be
    // keyed on the parent's raw Message-ID: for any thread deeper than
    // two messages that id differs from the inbound-path root, so the
    // sent copy fragmented into its own 1-message conversation.
    let mut thread_id: Option<String> = None;
    if let Some(irt) = &parts.in_reply_to
        && let Ok(resp) = state.core.find_thread_by_message_id(user, irt).await
    {
        thread_id = resp.thread_id;
    }
    let thread_id = match thread_id {
        Some(tid) => tid,
        None => parts
            .in_reply_to
            .clone()
            .unwrap_or_else(|| message_id.to_string()),
    };

    let (local, domain) = match user.split_once('@') {
        Some(v) => v,
        None => {
            tracing::warn!(%user, "mirror_send: malformed user address, skipping");
            return;
        }
    };
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let maildir_path = format!("{maildir_root}/{domain}/{local}");
    let store = MaildirStore;
    let blob_ref = match store.deliver_batch(&maildir_path, &[envelope]).await {
        Ok(ids) if !ids.is_empty() => {
            // sent copy counts against the sender's own quota
            let uk = format!("mailrs:quota:{}:used_bytes", user.to_lowercase());
            let n = envelope.len() as i64;
            let _ = crate::handlers::kevy_util::with_kevy(move |c| {
                c.incr_by(uk.as_bytes(), n).map_err(std::io::Error::other)?;
                Ok(())
            });
            ids[0].0.clone()
        }
        Ok(_) => {
            // Empty-Ok used to fall through as `blob_ref = ""`, silently.
            // Prod evidence 2026-07-24: a Jul 2 send hit this branch and
            // left a phantom row in Sent — no maildir file, size=0,
            // "(no text content)" in the UI, never enqueued, never
            // delivered. Treat it as the failure it is: warn AND use
            // the same synthetic ref the Err branch uses so downstream
            // has SOMETHING traceable to grep for.
            tracing::warn!(
                %user, %message_id,
                "mirror_send: deliver_batch returned no ids, using synthetic blob_ref"
            );
            format!("kevy:{message_id}")
        }
        Err(e) => {
            tracing::warn!(err = %e, %user, "mirror_send: maildir write failed, using synthetic blob_ref");
            format!("kevy:{message_id}")
        }
    };

    // Attach the maildir blob to the Send row, which was written during
    // the enqueue and could not know this yet: the file is produced here,
    // afterwards. Resend re-enqueues these bytes and re-edit parses them
    // back into compose fields, so without this both buttons have nothing
    // to act on (RFC 20260730-send-status S2).
    //
    // A synthetic `kevy:` ref means the maildir write failed — the bytes
    // are not on disk, so it is recorded as-is rather than pretending a
    // file exists. Resend has to refuse on those rather than re-enqueue
    // an envelope it cannot read.
    if !is_draft {
        let user_c = user.to_string();
        let send_id = message_id.to_string();
        let blob_c = blob_ref.clone();
        if let Err(code) = with_kevy(move |c| {
            mailrs_core_sidestate::families::send::set_envelope_ref(c, &user_c, &send_id, &blob_c)
        }) {
            tracing::warn!(
                %user, %message_id, ?code,
                "mirror_send: envelope_ref not attached — resend and re-edit \
                 will be unavailable for this send"
            );
        }
    }

    // Mark as read (sent) or draft in the maildir tag.
    if !blob_ref.is_empty() && !blob_ref.starts_with("kevy:") {
        let flag = if is_draft {
            mailrs_message_store::Flag::Draft
        } else {
            mailrs_message_store::Flag::Seen
        };
        let id = mailrs_message_store::MessageId(blob_ref.clone());
        if let Err(e) = store.mark_processed(&maildir_path, &id, &[flag]).await {
            tracing::debug!(err = %e, "mirror_send: mark_processed failed, continuing");
        }
    }

    let recipients_csv = parts.to.join(", ");
    let flags = if is_draft { 0b0001_0000 } else { 0b0000_0001 };
    let wire = MessageWire {
        id: 0,
        mailbox_id: 0,
        uid: 0,
        blob_ref: blob_ref.clone(),
        sender: user.to_string(),
        recipients: recipients_csv.clone(),
        subject: parts.subject.clone(),
        date: now,
        internal_date: now,
        size: envelope.len() as u32,
        flags,
        message_id: message_id.to_string(),
        in_reply_to: parts.in_reply_to.clone().unwrap_or_default(),
        // the user's own outbound copy — sender authentication is not a
        // meaningful question for mail we are sending ourselves.
        sender_trust: String::new(),
        thread_id: thread_id.clone(),
        modseq: 0,
        user_address: user.to_string(),
    };
    let wire_json = match serde_json::to_string(&wire) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(err = %e, "mirror_send: wire serialize failed");
            return;
        }
    };

    let req = DeliverMessageRequest {
        message_id: message_id.to_string(),
        subject: parts.subject.clone(),
        senders_csv: user.to_string(),
        latest_date: now,
        latest_preview: build_preview(parts),
        category: "inbox".to_string(),
        unread: false,
        uid: 0,
        payload_wire_json: wire_json,
    };
    if let Err(e) = state.core.deliver_message(user, &thread_id, &req).await {
        tracing::warn!(err = %e, %user, %thread_id, "mirror_send: fastcore deliver_message failed");
    }

    // Contacts autocomplete refresh — union of to+cc+bcc.
    let contact_targets: Vec<String> = parts
        .to
        .iter()
        .chain(parts.cc.iter())
        .chain(parts.bcc.iter())
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .collect();
    if !contact_targets.is_empty() {
        let user_owned = user.to_string();
        let now_ts = now_secs();
        let _ = with_kevy(move |c| {
            let key = format!("mailrs:user:{user_owned}:contacts");
            let ts_key = format!("mailrs:user:{user_owned}:contacts:ts");
            for raw in &contact_targets {
                let addr = extract_addr(raw);
                if addr.is_empty() {
                    continue;
                }
                let val = if raw.trim() != addr {
                    raw.trim().to_string()
                } else {
                    addr.clone()
                };
                c.hset(key.as_bytes(), &[(addr.as_bytes(), val.as_bytes())])
                    .map_err(std::io::Error::other)?;
                // Track last-used ts in a companion zset so we can
                // evict the least-recently-emailed contacts once the
                // set grows past a soft cap. Without this the hash
                // grows unbounded.
                c.zadd(ts_key.as_bytes(), &[(now_ts as f64, addr.as_bytes())])
                    .map_err(std::io::Error::other)?;
            }
            // Enforce a 2000-entry cap. If the zset exceeds it, drop
            // the oldest entries from both the hash and the zset.
            let size = c.zcard(ts_key.as_bytes()).map_err(std::io::Error::other)?;
            const CAP: usize = 2000;
            if size > CAP {
                let overflow = (size - CAP) as i64;
                let old = c
                    .zrange(ts_key.as_bytes(), 0, overflow - 1)
                    .map_err(std::io::Error::other)?;
                let old_refs: Vec<&[u8]> = old.iter().map(|v| v.as_slice()).collect();
                if !old_refs.is_empty() {
                    c.hdel(key.as_bytes(), &old_refs)
                        .map_err(std::io::Error::other)?;
                    c.zrem(ts_key.as_bytes(), &old_refs)
                        .map_err(std::io::Error::other)?;
                }
            }
            Ok(())
        });
    }
}
