//! Which attachments a reply, forward or redraft carries over, and where
//! it hangs in the thread.

use std::sync::Arc;

use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::compose::*;

/// Prepend the original message onto a forward's body/html and copy
/// its attachments across, so a compose whose isBackendForward path
/// sent only the user's leading line actually carries the forwarded
/// content the recipient expects to see.
///
/// The frontend contract (reply-box.tsx `isBackendForward`) is: when
/// `forward_message_id` is set, ship only the typed text — backend
/// inlines the original. `build_rfc5322` on its own ignores that
/// field, which is why every forward before 2.9.38 arrived as just
/// the leading line (no body, no attachments).
///
/// Best-effort: a lookup failure logs a warning and passes the send
/// through unmodified rather than fail — the compose already exists
/// on the caller's side and the operator can retry manually.
pub(crate) async fn inline_forward_content(
    state: &Arc<WebState>,
    user: &str,
    parts: &mut ComposeParts,
) {
    let mid = parts
        .forward_message_id
        .as_deref()
        .filter(|s| !s.is_empty());
    let uid = parts.forward_attachments_from;
    if mid.is_none() && uid.is_none() {
        return;
    }
    // Message-ID first, uid as the fallback. Not the other way round: the
    // Message-ID identifies the mail across mailboxes, while the uid is
    // per-mailbox. But a message with no Message-ID header has only a uid,
    // and that case is exactly why the client sends both.
    let wire = match resolve_forward_source(state, user, mid, uid).await {
        Some(w) => w,
        None => return,
    };
    let raw = match super::messages::read_maildir_bytes_pub(user, &wire.blob_ref).await {
        Ok(r) => r,
        Err(status) => {
            tracing::warn!(
                status = ?status, %user, blob_ref = %wire.blob_ref,
                "forward inline: raw fetch failed"
            );
            return;
        }
    };
    let (orig_text, orig_html, _atts) = super::conversations::parse_body(&raw);

    // Attachments: walk the parsed MIME tree, skip the multipart
    // wrappers and the text/plain + text/html body parts (those went
    // into orig_text/orig_html above), and push everything else onto
    // parts.attachments. build_rfc5322 wraps the outer message in
    // multipart/mixed when parts.attachments is non-empty and lays
    // each attachment out with its filename + content-type + base64.
    //
    // A part is a body part when it's text/plain or text/html AND
    // does not have `Content-Disposition: attachment` (inline is a
    // body; attachment-disposition text is treated as a file). Same
    // rule the retired monolith send/text.rs used.
    parts.attachments.extend(attachments_from_envelope(&raw));

    let user_text = std::mem::take(&mut parts.body);
    parts.body = match orig_text.as_deref() {
        Some(text) => format!("{user_text}\n\n---------- Forwarded message ----------\n{text}"),
        None => user_text.clone(),
    };

    // HTML: if the compose didn't submit HTML, synth a paragraph from
    // the plain text so a divider + forwarded HTML can be appended.
    let user_html_owned = std::mem::take(&mut parts.html_body);
    let user_html = if user_html_owned.is_empty() {
        format!("<p>{}</p>", user_text.replace('\n', "<br>"))
    } else {
        user_html_owned
    };
    parts.html_body = if let Some(html) = orig_html {
        format!(
            "{user_html}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><div style=\"color:#555\">{html}</div>"
        )
    } else if let Some(text) = orig_text.as_deref() {
        let escaped = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>");
        format!(
            "{user_html}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><pre style=\"font-family:sans-serif;white-space:pre-wrap\">{escaped}</pre>"
        )
    } else {
        user_html
    };
}

/// Carry the attachments of the send being repaired onto this compose.
///
/// **Fails the send rather than degrading it.** The forward path is
/// best-effort on purpose — a lookup failure there means the recipient
/// sees less quoted text, and the mail is still the message the user
/// wrote. Here a failure means a mail the user believes carries five
/// images arrives with none, which is the exact silent loss this design
/// exists to prevent.
///
/// `keep` names indices into `attachments_from_envelope`'s output. Out-of
/// -range indices are rejected: it means the caller and the server
/// disagree about what the parts are, and guessing which was meant is how
/// the wrong file gets attached.
pub(crate) async fn inline_redraft_attachments(
    user: &str,
    redraft_of: Option<&str>,
    keep: Option<&[usize]>,
    parts: &mut ComposeParts,
) -> Result<(), StatusCode> {
    let Some(send_id) = redraft_of.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let raw = super::sends::envelope_bytes(user, send_id).await?;
    let carried = attachments_from_envelope(&raw);
    let have = carried.len();
    let selected = select_carried(carried, keep).map_err(|i| {
        tracing::warn!(
            %user, %send_id, index = i, have,
            "redraft_keep names an attachment the envelope does not have"
        );
        StatusCode::BAD_REQUEST
    })?;
    // Ahead of anything newly uploaded, so the order the user saw in the
    // composer is the order that goes out.
    let fresh = std::mem::take(&mut parts.attachments);
    parts.attachments = selected;
    parts.attachments.extend(fresh);
    Ok(())
}

/// Pick the carried attachments the caller asked to keep.
///
/// `None` keeps all; `Some(&[])` keeps none — the caller removed every
/// one. Returns the offending index when a selection names a part that is
/// not there, because the alternative is attaching a different file than
/// the user saw.
pub(crate) fn select_carried(
    carried: Vec<Attachment>,
    keep: Option<&[usize]>,
) -> Result<Vec<Attachment>, usize> {
    let Some(idx) = keep else {
        return Ok(carried);
    };
    let mut picked = Vec::with_capacity(idx.len());
    for &i in idx {
        picked.push(carried.get(i).ok_or(i)?.clone());
    }
    Ok(picked)
}

/// Every part of `raw` that is a file rather than a body, ready to hand
/// back to `build_rfc5322`.
///
/// Shared by forward (quote an existing message) and redraft (repair a
/// send that failed), which are the two paths that rebuild an outgoing
/// message from a stored one. It was inline in the forward path until
/// redraft needed it; copying it would have put two answers to "which
/// parts are body, which are files" in the tree, and the copies drift
/// (`.claude/rules/feedback-two-impls-need-a-contract-test`).
///
/// A part is a body part when it is text/plain or text/html **and** is
/// not `Content-Disposition: attachment` — an attachment-disposition
/// text part is a file someone attached, not the message body. Same
/// rule the retired monolith's send/text.rs used.
pub(crate) fn attachments_from_envelope(raw: &[u8]) -> Vec<Attachment> {
    let parsed = mailrs_mime::parse(raw);
    let mut out = Vec::new();
    for part in parsed.walk() {
        if part.content_type.is_multipart() {
            continue;
        }
        let mt = part.content_type.mime_type();
        let is_body_part = (mt == "text/plain" || mt == "text/html")
            && part
                .disposition
                .as_ref()
                .map(|d| !d.is_attachment())
                .unwrap_or(true);
        if is_body_part {
            continue;
        }
        out.push(Attachment {
            filename: part
                .attachment_filename()
                .map(String::from)
                .unwrap_or_else(|| "attachment".to_string()),
            content_type: mt,
            bytes: part.body.to_vec(),
        });
    }
    out
}

/// The Message-ID a reply into this thread should reference.
///
/// The newest message, which is what replying to a conversation means. Only
/// messages that have an id: one without a Message-ID cannot be referenced,
/// and picking it would produce `In-Reply-To: <>` — a header that threads
/// nothing and is worse than none, because it looks like an answer.
pub(crate) fn newest_referencable(
    items: &[mailrs_core_api::method::message::MessageWire],
) -> Option<String> {
    items
        .iter()
        .filter(|w| !w.message_id.is_empty())
        .max_by_key(|w| w.internal_date)
        .map(|w| w.message_id.clone())
}

/// Fill in `in_reply_to` from the thread when the client did not send one.
///
/// Without a parent Message-ID there is no `In-Reply-To` header, so the
/// recipient's client files the reply as a new conversation and our own
/// mirror cannot find the thread either — it falls back to the new
/// message's own id and the sent copy becomes a thread of one. That is what
/// happened to a reply on 2026-07-30.
///
/// The newest message in the thread is the parent, which is what a reply
/// means. Best-effort: a lookup failure leaves the send unthreaded rather
/// than refusing it, because an unthreaded reply that arrives beats a reply
/// that does not.
pub(crate) async fn infer_in_reply_to(state: &Arc<WebState>, user: &str, parts: &mut ComposeParts) {
    if parts.in_reply_to.as_deref().is_some_and(|s| !s.is_empty()) {
        return;
    }
    let Some(tid) = parts
        .reply_to_thread_id
        .as_deref()
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let resp = match state.core.list_thread_messages(user, tid).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, %user, thread_id = %tid, "reply threading: thread lookup failed");
            return;
        }
    };
    let parent = newest_referencable(&resp.items);
    match parent {
        Some(mid) => {
            tracing::info!(
                %user, thread_id = %tid, parent = %mid,
                "reply threading: in_reply_to inferred from the thread"
            );
            parts.in_reply_to = Some(mid);
        }
        None => {
            tracing::warn!(
                %user, thread_id = %tid,
                "reply threading: thread has no message with a Message-ID"
            );
        }
    }
}

/// The message a forward is quoting, by Message-ID or by uid.
///
/// Returns `None` after logging; the caller is best-effort by design.
pub(crate) async fn resolve_forward_source(
    state: &Arc<WebState>,
    user: &str,
    mid: Option<&str>,
    uid: Option<u32>,
) -> Option<mailrs_core_api::method::message::MessageWire> {
    if let Some(mid) = mid {
        match state.core.find_by_message_id_for_user(user, mid).await {
            Ok(w) => return Some(w),
            Err(e) => {
                tracing::warn!(
                    error = %e, %user, message_id = %mid,
                    "forward inline: message-id lookup failed, trying uid"
                );
            }
        }
    }
    let uid = uid?;
    match super::messages::resolve_message(state, user, uid).await {
        Ok(w) => Some(w),
        Err(status) => {
            tracing::warn!(?status, %user, uid, "forward inline: uid lookup failed");
            None
        }
    }
}

#[cfg(test)]
mod carried_attachment_tests {
    use crate::handlers::compose::Attachment;
    use crate::handlers::compose_attach::{attachments_from_envelope, select_carried};

    fn att(filename: &str) -> Attachment {
        Attachment {
            filename: filename.into(),
            content_type: "application/octet-stream".into(),
            bytes: vec![1, 2, 3],
        }
    }

    /// Absent and empty mean opposite things: nothing removed versus
    /// everything removed. Collapsing them re-attaches files the user
    /// deleted, which they would only find out about after sending.
    #[test]
    fn keeping_nothing_is_not_the_same_as_saying_nothing() {
        let carried = vec![att("a.png"), att("b.pdf")];
        assert_eq!(select_carried(carried.clone(), None).unwrap().len(), 2);
        assert!(select_carried(carried, Some(&[])).unwrap().is_empty());
    }

    /// Indices, not filenames — two parts can both be `image.png`, and a
    /// name-keyed selection would silently pick whichever came first.
    #[test]
    fn identically_named_parts_are_still_told_apart() {
        let carried = vec![att("image.png"), att("image.png"), att("notes.txt")];
        let picked = select_carried(carried, Some(&[1, 2])).unwrap();
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[1].filename, "notes.txt");
    }

    /// An out-of-range index means the caller and the server disagree
    /// about what the parts are. Guessing which was meant is how the wrong
    /// file gets attached.
    #[test]
    fn an_index_the_envelope_does_not_have_is_refused() {
        assert_eq!(select_carried(vec![att("a.png")], Some(&[0, 4])), Err(4));
    }

    /// The rule forward and redraft now share. A text part with
    /// `Content-Disposition: attachment` is a file someone attached, not
    /// the message body — treating it as body would drop it from the mail.
    #[test]
    fn a_text_part_marked_as_an_attachment_is_a_file_not_the_body() {
        let raw = concat!(
            "From: me@x.com\r\n",
            "Subject: t\r\n",
            "Content-Type: multipart/mixed; boundary=b\r\n",
            "\r\n",
            "--b\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "the body\r\n",
            "--b\r\n",
            "Content-Type: text/plain\r\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\r\n",
            "\r\n",
            "attached text\r\n",
            "--b--\r\n",
        );
        let found = attachments_from_envelope(raw.as_bytes());
        assert_eq!(found.len(), 1, "the body part must not be carried");
        assert_eq!(found[0].filename, "notes.txt");
    }
}

#[cfg(test)]
mod reply_threading_tests {
    use super::newest_referencable;
    use mailrs_core_api::method::message::MessageWire;

    fn msg(message_id: &str, internal_date: i64) -> MessageWire {
        MessageWire {
            id: 0,
            mailbox_id: 0,
            uid: 0,
            blob_ref: String::new(),
            sender: "a@x.com".into(),
            recipients: String::new(),
            subject: String::new(),
            date: internal_date,
            internal_date,
            size: 0,
            flags: 0,
            message_id: message_id.into(),
            in_reply_to: String::new(),
            sender_trust: String::new(),
            thread_id: "t".into(),
            modseq: 0,
            user_address: "me@x.com".into(),
        }
    }

    /// Replying to a conversation means replying to its latest message, and
    /// the thread's messages do not arrive in date order.
    #[test]
    fn the_newest_message_is_the_parent() {
        let items = vec![
            msg("old@x.com", 100),
            msg("newest@x.com", 300),
            msg("middle@x.com", 200),
        ];
        assert_eq!(newest_referencable(&items).as_deref(), Some("newest@x.com"));
    }

    /// A message with no Message-ID cannot be referenced. Choosing it would
    /// emit `In-Reply-To: <>`, which threads nothing while looking like an
    /// answer — so it is skipped even when it is the newest.
    #[test]
    fn a_message_without_an_id_is_never_the_parent() {
        let items = vec![msg("has-id@x.com", 100), msg("", 999)];
        assert_eq!(newest_referencable(&items).as_deref(), Some("has-id@x.com"));
    }

    /// Nothing to reference: the send goes out unthreaded rather than with a
    /// header pointing at nothing.
    #[test]
    fn no_referencable_message_yields_none() {
        assert_eq!(newest_referencable(&[]), None);
        assert_eq!(newest_referencable(&[msg("", 1), msg("", 2)]), None);
    }
}
