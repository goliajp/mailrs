//! The pieces of a text send decided before the MIME is built: whether the
//! request is acceptable, the quoted reply body, and the forwarded body.

use crate::message_util;

use super::super::super::WebState;
use super::super::SendMessageRequest;

/// Why the request cannot be sent as given, or `None` when it can.
pub(super) fn request_error(req: &SendMessageRequest) -> Option<String> {
    if req.to.is_empty() {
        return Some("to is required".into());
    }

    let total_recipients = req.to.len() + req.cc.len() + req.bcc.len();
    if total_recipients > crate::web::MAX_RECIPIENTS {
        return Some(format!(
            "too many recipients (max {})",
            crate::web::MAX_RECIPIENTS
        ));
    }

    if req.body.len() > crate::web::MAX_EMAIL_BODY_LEN {
        return Some("message body too large".into());
    }

    if req.subject.len() > crate::web::MAX_ADMIN_FIELD_LEN {
        return Some("subject too long".into());
    }
    None
}

/// The body with the original message quoted under it when this is a
/// reply whose original can be read; the body as typed otherwise.
pub(super) async fn body_with_quote(
    state: &WebState,
    from: &str,
    in_reply_to: Option<&str>,
    body: &str,
) -> String {
    match (in_reply_to, state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            quoted_reply(state, mb_store, from, reply_to, body)
                .await
                .unwrap_or_else(|| body.to_string())
        }
        _ => body.to_string(),
    }
}

async fn quoted_reply(
    state: &WebState,
    mb_store: &mailrs_mailbox::PgMailboxStore,
    from: &str,
    reply_to: &str,
    body: &str,
) -> Option<String> {
    let orig = mb_store
        .find_message_by_message_id(from, reply_to)
        .await
        .ok()
        .flatten()?;
    let raw_orig =
        message_util::read_message_raw(&state.maildir_root, from, &orig.maildir_id).await?;
    let (text_body, _, _) = message_util::parse_message(&raw_orig);
    let text = text_body?;
    let sender = message_util::decode_header(&orig.sender);
    let date = chrono::DateTime::from_timestamp(orig.internal_date, 0)
        .map(|dt| dt.format("%a, %d %b %Y %H:%M").to_string())
        .unwrap_or_default();
    let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
    Some(format!("{}\n\nOn {date}, {sender} wrote:\n{quoted}", body))
}

/// The text and HTML bodies of a forward: the user's message first, the
/// forwarded content after it.
pub(super) fn forward_bodies(
    req: &SendMessageRequest,
    orig_text: &Option<String>,
    orig_html: &Option<String>,
) -> (String, Option<String>) {
    // prepend user's message before the forwarded content
    let user_text = req.body.clone();
    let fwd_text = if let Some(text) = orig_text {
        format!("{user_text}\n\n---------- Forwarded message ----------\n{text}")
    } else {
        user_text.clone()
    };
    let user_html_fallback = format!("<p>{}</p>", user_text.replace('\n', "<br>"));
    let user_html_str = req.html_body.as_deref().unwrap_or(&user_html_fallback);
    let fwd_html = if let Some(html) = orig_html {
        Some(format!(
            "{user_html_str}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><div style=\"color:#555\">{html}</div>"
        ))
    } else if let Some(text) = orig_text {
        let escaped = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>");
        Some(format!(
            "{user_html_str}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><pre style=\"font-family:sans-serif;white-space:pre-wrap\">{escaped}</pre>"
        ))
    } else {
        req.html_body.clone()
    };
    (fwd_text, fwd_html)
}
