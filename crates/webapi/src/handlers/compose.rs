//! Building an outgoing message: the RFC 5322 bytes, the attachment
//! carrying rules, and the checks on who a message may claim to be from.
//!
//! Split out of `prefs.rs` on 2026-08-02, which had grown to 2,148 lines
//! holding the entire send path next to drafts, templates and signatures.

use std::sync::Arc;

use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::prefs::now_secs;

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

/// RFC 5322 date string in the shape smtpd wants.
pub(crate) fn rfc5322_date(epoch: i64) -> String {
    // Manual format so we don't pull in chrono here — the outbound
    // queue consumer re-parses this defensively anyway.
    // Sat, 02 Jul 2026 12:34:56 +0000
    static WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Simple date math — fine within the current epoch range.
    let secs = epoch.max(0) as u64;
    let mut days = secs / 86_400;
    let sec_of_day = secs % 86_400;
    let hour = (sec_of_day / 3600) as u32;
    let minute = ((sec_of_day % 3600) / 60) as u32;
    let second = (sec_of_day % 60) as u32;
    // 1970-01-01 was Thursday (index 4)
    let weekday = WEEKDAYS[((days + 4) % 7) as usize];
    let mut year: u32 = 1970;
    while {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let ydays = if leap { 366 } else { 365 };
        days >= ydays
    } {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        days -= if leap { 366 } else { 365 };
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: usize = 0;
    while month < 12 && days >= month_lengths[month] {
        days -= month_lengths[month];
        month += 1;
    }
    let day = days + 1;
    format!(
        "{weekday}, {day:02} {mon} {year} {hour:02}:{minute:02}:{second:02} +0000",
        mon = MONTHS[month],
    )
}

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

/// Build a complete RFC 5322 envelope from parsed compose parts.
///
/// Transport-safe by construction:
/// - subject: RFC 2047 encoded-word
/// - attachment filenames: RFC 2231 (`filename*=UTF-8''<pct>`) so
///   non-ASCII filenames survive strict MTAs and older MUAs
/// - text/plain + text/html bodies: base64 (safe on non-8BITMIME hops,
///   avoids quoted-printable blow-up on CJK)
/// - attachment payload: base64 with 76-column line wrapping
pub(crate) fn build_rfc5322(parts: &ComposeParts, from: &str) -> (String, Vec<u8>) {
    let mid_local = random_hex(8);
    let mid_host = from.split('@').nth(1).unwrap_or("localhost");
    let message_id = format!("{mid_local}@{mid_host}");
    let date = rfc5322_date(now_secs());

    let has_attachments = !parts.attachments.is_empty();
    let has_html = !parts.html_body.is_empty();

    let mixed_boundary = format!("----=Mixed_{}", random_hex(6));
    let alt_boundary = format!("----=Alt_{}", random_hex(6));

    let mut out = String::new();
    out.push_str(&format!("Date: {date}\r\n"));
    out.push_str(&format!("From: {from}\r\n"));
    if !parts.to.is_empty() {
        out.push_str(&format!("To: {}\r\n", parts.to.join(", ")));
    }
    if !parts.cc.is_empty() {
        out.push_str(&format!("Cc: {}\r\n", parts.cc.join(", ")));
    }
    let encoded_subject = mailrs_rfc2047::encode(&parts.subject);
    out.push_str(&format!("Subject: {encoded_subject}\r\n"));
    out.push_str(&format!("Message-ID: <{message_id}>\r\n"));
    out.push_str("MIME-Version: 1.0\r\n");
    if let Some(ref irt) = parts.in_reply_to {
        out.push_str(&format!("In-Reply-To: <{irt}>\r\n"));
        out.push_str(&format!("References: <{irt}>\r\n"));
    }

    // Assemble text/alternative/mixed structure. We always emit an outer
    // Content-Type in the top-level header, then the body parts. When
    // there are attachments the outer is multipart/mixed; the first
    // inner part is either text/plain or multipart/alternative.
    let body_section = if has_html {
        let mut s = String::new();
        s.push_str(&format!(
            "Content-Type: multipart/alternative; boundary=\"{alt_boundary}\"\r\n\r\n"
        ));
        s.push_str(&format!("--{alt_boundary}\r\n"));
        s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.body.as_bytes()));
        s.push_str(&format!("\r\n--{alt_boundary}\r\n"));
        s.push_str("Content-Type: text/html; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.html_body.as_bytes()));
        s.push_str(&format!("\r\n--{alt_boundary}--\r\n"));
        s
    } else {
        let mut s = String::new();
        s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.body.as_bytes()));
        s
    };

    if has_attachments {
        out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{mixed_boundary}\"\r\n\r\n"
        ));
        out.push_str(&format!("--{mixed_boundary}\r\n"));
        out.push_str(&body_section);
    } else {
        out.push_str(&body_section);
    }

    let mut bytes = out.into_bytes();

    if has_attachments {
        for att in &parts.attachments {
            let mut part = String::new();
            let ct_name = mailrs_rfc2231::encode_param("name", &att.filename);
            let cd_name = mailrs_rfc2231::encode_param("filename", &att.filename);
            part.push_str(&format!("\r\n--{mixed_boundary}\r\n"));
            part.push_str(&format!(
                "Content-Type: {ct}; {ct_name}\r\n",
                ct = att.content_type,
            ));
            part.push_str(&format!("Content-Disposition: attachment; {cd_name}\r\n"));
            part.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
            part.push_str(&base64_wrap(&att.bytes));
            bytes.extend_from_slice(part.as_bytes());
        }
        bytes.extend_from_slice(format!("\r\n--{mixed_boundary}--\r\n").as_bytes());
    }

    (message_id, bytes)
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
    /// these, re-edit parses them back. Empty until the mirror has
    /// written the file, which happens after this call; S2 fills it in.
    pub envelope_ref: &'a str,
    pub resent_from: Option<&'a str>,
}

/// Take the first ~120 chars of `body` (or html-stripped `html_body`
/// if body is empty) as the preview shown in conversation lists.
pub(crate) fn build_preview(parts: &ComposeParts) -> String {
    let src = if !parts.body.is_empty() {
        parts.body.clone()
    } else if !parts.html_body.is_empty() {
        html2text::from_read(parts.html_body.as_bytes(), 80).unwrap_or_default()
    } else {
        String::new()
    };
    // The same rule the inbound drain uses. This used to replace CR and
    // LF with spaces and cut at 120, which on html2text output — blank
    // lines, non-breaking spaces — could spend the whole line on
    // whitespace, and left a sent thread reading differently from a
    // received one in the same list.
    mailrs_clean::preview_line(&src, 120)
}

/// Extract the addr-spec (`user@host`) from an RFC 5322 mailbox token.
/// Mirrors sender.rs's helper — kept here so webapi doesn't depend on
/// the fastcore-sender bin crate.
pub(crate) fn extract_addr(raw: &str) -> String {
    let t = raw.trim();
    if let Some(start) = t.rfind('<')
        && let Some(end) = t.rfind('>')
        && end > start
    {
        return t[start + 1..end].trim().to_string();
    }
    t.to_string()
}

/// Return `Ok(())` iff `from` matches the authed user's own address
/// or any entry in their effective_permissions.send_as list.
/// Otherwise `Err(FORBIDDEN)` — this stops any authenticated user
/// from spoofing arbitrary From: (in particular, arbitrary domains).
pub(crate) async fn ensure_from_allowed(
    state: &Arc<WebState>,
    user: &str,
    from: &str,
) -> Result<(), StatusCode> {
    if from == user {
        return Ok(());
    }
    let perms = state
        .core
        .effective_permissions(user)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if perms.is_super || perms.send_as.iter().any(|s| s == from) {
        return Ok(());
    }
    tracing::warn!(%user, %from, "send blocked: from not in send_as allowlist");
    Err(StatusCode::FORBIDDEN)
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

    /// A fresh send starts its own conversation, so the thread is its own
    /// Message-ID — the same derivation `mirror_send_to_sender_view` uses,
    /// asserted here so the Send row and the thread copy cannot disagree
    /// about which conversation a send belongs to.
    #[test]
    fn a_new_send_threads_on_its_own_message_id() {
        let parts = parts_for("hello", &["a@y.com"], None);
        let (message_id, _envelope) = build_rfc5322(&parts, &parts.from);
        let meta = SendMeta {
            message_id: &message_id,
            thread_id: parts.in_reply_to.as_deref().unwrap_or(&message_id),
            subject: &parts.subject,
            to: &parts.to,
            cc: &parts.cc,
            envelope_ref: "",
            resent_from: None,
        };
        assert_eq!(meta.thread_id, message_id);
    }

    #[test]
    fn a_reply_threads_on_its_parent() {
        let parts = parts_for("re: hello", &["a@y.com"], Some("parent@y.com"));
        let (message_id, _envelope) = build_rfc5322(&parts, &parts.from);
        let meta = SendMeta {
            message_id: &message_id,
            thread_id: parts.in_reply_to.as_deref().unwrap_or(&message_id),
            subject: &parts.subject,
            to: &parts.to,
            cc: &parts.cc,
            envelope_ref: "",
            resent_from: None,
        };
        assert_eq!(meta.thread_id, "parent@y.com");
        assert_ne!(meta.thread_id, message_id);
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
        };
        assert_eq!(meta.to.len(), 2);
        assert_eq!(meta.cc.len(), 1);
    }
}
