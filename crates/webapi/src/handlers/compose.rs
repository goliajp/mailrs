//! Building an outgoing message: the RFC 5322 bytes, the attachment
//! carrying rules, and the checks on who a message may claim to be from.
//!
//! Split out of `prefs.rs` on 2026-08-02, which had grown to 2,148 lines
//! holding the entire send path next to drafts, templates and signatures.

use std::sync::Arc;

use axum::http::StatusCode;

use crate::WebState;

/// One entry parsed out of the compose form.
#[derive(Debug, Default)]
pub(crate) struct ComposeParts {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub html_body: String,
    pub in_reply_to: Option<String>,
    pub forward_message_id: Option<String>,
    /// uid of the message to forward, for the case where it has no
    /// Message-ID to look it up by.
    pub forward_attachments_from: Option<u32>,
    /// Thread to resolve a parent Message-ID from when `in_reply_to` is
    /// missing.
    pub reply_to_thread_id: Option<String>,
    pub attachments: Vec<Attachment>,
    /// Unix epoch seconds to send at; None / past = send now (G13).
    pub scheduled_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    /// Set for an image the HTML body shows through `cid:`; such a part
    /// goes beside the HTML in `multipart/related`, not into the files.
    pub content_id: Option<String>,
}

pub(crate) use super::compose_build::{breaks_a_header_line, build_rfc5322};

/// Longest hold a client may ask for; Gmail's longest undo window.
pub const MAX_UNDO_SECONDS: u32 = 30;

/// When to release a send. A send scheduled for later keeps its time; an
/// immediate one with an undo window is held that long, through the same
/// scheduled queue, because the sender claims a job the moment it reaches
/// the pending list and an undo that loses that race takes nothing back.
pub fn hold_for_undo(
    scheduled_at: Option<i64>,
    undo_seconds: Option<u32>,
    now: i64,
) -> Option<i64> {
    scheduled_at.or_else(|| {
        undo_seconds
            .filter(|s| *s > 0)
            .map(|s| now + i64::from(s.min(MAX_UNDO_SECONDS)))
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct SendRequest {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub html_body: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub forward_message_id: Option<String>,
    /// uid of the message being forwarded. The client sends this alongside
    /// `forward_message_id`, and it is the **only** identifier when the
    /// original has no Message-ID header — a forward of one of those used
    /// to arrive as the typed line alone, because this handler did not name
    /// the field and the monolith's did
    /// (`crates/server/src/web/mail/send/text.rs`).
    #[serde(default)]
    pub forward_attachments_from: Option<u32>,
    /// The conversation this is a reply inside.
    ///
    /// A threading fallback, not a replacement for `in_reply_to`: when that
    /// is absent this resolves the parent's Message-ID from the thread, so a
    /// reply stays threaded even though the client did not name a parent
    /// message.
    ///
    /// It exists because the client can drop `in_reply_to` and nothing
    /// notices. A reply with an attachment arrived unthreaded on 2026-07-30
    /// while two attachment-less replies the same day were fine; the draft
    /// round-trip is one way to lose it (the stored draft keeps
    /// `reply_to_thread_id` but the client's `ComposeDraftSource` never read
    /// it back). Every surface that can reply knows which conversation it is
    /// in, so keying the fallback on the thread covers all of them at once.
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
    #[serde(default)]
    pub scheduled_at: Option<i64>,
    /// Seconds to hold an immediate send so the person can still take it
    /// back. See [`hold_for_undo`].
    #[serde(default)]
    pub undo_seconds: Option<u32>,
    /// The failed send this repairs. Its attachments are re-extracted
    /// here; they never travelled to the browser.
    #[serde(default)]
    pub redraft_of: Option<String>,
    /// Which carried attachments to keep, as indices into the original
    /// envelope's attachment list. Absent keeps all.
    ///
    /// Indices, not filenames: two parts can both be `image.png`, and the
    /// wrong one would be dropped with no way to tell.
    #[serde(default)]
    pub redraft_keep: Option<Vec<usize>>,
    /// The saved draft this sends: its files go first, and it is deleted
    /// once the message is queued (`draft_send`).
    #[serde(default)]
    pub draft_id: Option<i64>,
    /// The team conversation this answers, when it is a team inbox reply:
    /// the reply is claimed before it is queued, and refused with 409 when
    /// another member answered since the composer opened.
    #[serde(default)]
    pub shared: Option<crate::handlers::shared_inbox::claim::SharedReply>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct SendResponse {
    pub message_id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub(crate) use super::compose_date::rfc5322_date;

/// Encode a body buffer as base64 with 76-column line wrapping.
/// Universal-safe transport — no 8BITMIME dependence, no quoted-printable
/// pathological blow-up on CJK text.
pub(crate) fn base64_wrap(input: &[u8]) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(input);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 * 2);
    for chunk in encoded.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push_str("\r\n");
    }
    out
}

/// `from` as a From header value: a bare address as is, and a display
/// name quoted, or encoded when it is not ASCII.
pub(crate) fn from_header(from: &str) -> String {
    let address = mailrs_rfc5322::addr_spec(from);
    let name = from
        .trim()
        .rfind('<')
        .map(|open| from.trim()[..open].trim().trim_matches('"').trim())
        .unwrap_or("");
    if name.is_empty() || address == from.trim() {
        return address.to_string();
    }
    match mailrs_rfc2047::encode(name) {
        std::borrow::Cow::Borrowed(ascii) => {
            let quoted = ascii.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{quoted}\" <{address}>")
        }
        std::borrow::Cow::Owned(encoded) => format!("{encoded} <{address}>"),
    }
}

/// A `scheduled_at` form field → epoch seconds, or a rejection.
///
/// Empty means "not scheduling" — an untouched form field arrives as an
/// empty string, and that is not an error.
///
/// Anything else that is not an integer **is** an error. This used to be
/// `.parse().ok()`, which turned an unparseable value into `None`, and
/// `None` means send now: the web composer sent an ISO 8601 string here
/// for as long as scheduling existed, so every scheduled send went out
/// immediately and nothing anywhere said so. Silently doing the one thing
/// the user was trying to avoid is the worst available outcome, so a value
/// that cannot be read is a 400.
pub(crate) fn parse_scheduled_at(raw: &str) -> Result<Option<i64>, ()> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<i64>().map(Some).map_err(|_| ())
}

/// What the Send view renders, handed to [`enqueue_outbound_at`] so the
/// row lands in the same fallible step as the queue write.
pub(crate) struct SendMeta<'a> {
    pub message_id: &'a str,
    pub thread_id: &'a str,
    pub subject: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    /// Maildir blob holding the RFC 5322 bytes — resend re-enqueues
    /// these, re-edit parses them back. The core's sent copy.
    pub envelope_ref: &'a str,
    pub resent_from: Option<&'a str>,
    /// The agent key that made this send, when one did.
    pub key_id: Option<i64>,
}

/// Return `Ok(())` iff the authed user may send as `from`
/// (`EffectivePermissionsResponse::may_send_as`, which SMTP submission
/// also asks).
/// Otherwise `Err(FORBIDDEN)` — this stops any authenticated user
/// from spoofing arbitrary From: (in particular, arbitrary domains).
pub(crate) async fn ensure_from_allowed(
    state: &Arc<WebState>,
    user: &str,
    from: &str,
) -> Result<(), StatusCode> {
    // the value becomes a header line as it is
    if from.contains(['\r', '\n']) {
        return Err(StatusCode::BAD_REQUEST);
    }
    // a display name is the sender's to choose; the address is what is checked
    let from = mailrs_rfc5322::addr_spec(from);
    if from.eq_ignore_ascii_case(user) {
        return Ok(());
    }
    let perms = state
        .core
        .effective_permissions(user)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if perms.may_send_as(from) {
        return Ok(());
    }
    tracing::warn!(%user, %from, "send blocked: from not in send_as allowlist");
    Err(StatusCode::FORBIDDEN)
}

#[cfg(test)]
mod from_header_tests {
    use super::from_header;

    #[test]
    fn display_names_are_quoted_or_encoded() {
        assert_eq!(from_header("lihao@golia.jp"), "lihao@golia.jp");
        assert_eq!(from_header(" lihao@golia.jp "), "lihao@golia.jp");
        assert_eq!(
            from_header("LI HAO <lihao@golia.jp>"),
            "\"LI HAO\" <lihao@golia.jp>"
        );
        assert_eq!(
            from_header("\"Li, Hao\" <lihao@golia.jp>"),
            "\"Li, Hao\" <lihao@golia.jp>"
        );
        assert_eq!(
            from_header("李浩 <lihao@golia.jp>"),
            "=?UTF-8?B?5p2O5rWp?= <lihao@golia.jp>"
        );
        assert_eq!(from_header("<lihao@golia.jp>"), "lihao@golia.jp");
    }
}

#[cfg(test)]
mod send_meta_tests {
    use super::*;

    fn parts_for(subject: &str, to: &[&str], in_reply_to: Option<&str>) -> ComposeParts {
        ComposeParts {
            from: "me@x.com".into(),
            to: to.iter().map(|s| s.to_string()).collect(),
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            body: "body".into(),
            html_body: String::new(),
            in_reply_to: in_reply_to.map(String::from),
            forward_message_id: None,
            forward_attachments_from: None,
            reply_to_thread_id: None,
            attachments: Vec::new(),
            scheduled_at: None,
        }
    }

    /// The row a send returns 200 with has to describe every recipient
    /// the envelope went to, or the Send view under-reports who was
    /// mailed — and `partial` becomes unreachable for the case it exists
    /// for.
    #[test]
    fn the_row_covers_to_and_cc() {
        let mut parts = parts_for("hi", &["a@y.com", "b@y.com"], None);
        parts.cc = vec!["c@y.com".into()];
        let (message_id, _envelope) = build_rfc5322(&parts, &parts.from);
        let meta = SendMeta {
            message_id: &message_id,
            thread_id: &message_id,
            subject: &parts.subject,
            to: &parts.to,
            cc: &parts.cc,
            envelope_ref: "",
            resent_from: None,
            key_id: None,
        };
        assert_eq!(meta.to.len(), 2);
        assert_eq!(meta.cc.len(), 1);
    }
}
