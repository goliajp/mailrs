//! Reading a message body out of maildir and onto the wire.
//!
//! The header helpers live here rather than in a stone because they read
//! only what the thread view needs — Cc, Date — from the first bytes of a
//! file, and a full MIME parse per row is what this exists to avoid.

/// Wire shape UI expects for a message. Mirrors monolith's
/// `ThreadMessageResponse` — critical fields the UI reaches into
/// unconditionally (`attachments.length`, `text_body`, `html_body`,
/// `people`, `dates`, ...). Return default empty arrays / null when
/// the source `MessageWire` lacks the analysis fields.
#[derive(serde::Serialize)]
pub struct ThreadMessageResponse {
    /// Per-user unique, stable message identity. The client uses this
    /// as the React key for timeline items. `MessageWire.id` used to
    /// live here but was always 0 under the kevy-only architecture
    /// (no PG primary key), and duplicate keys drove undefined React
    /// reconciliation (see 2026-07-08 timeline-duplication reports).
    pub uid: u32,
    pub sender: String,
    /// Sender-authentication verdict from `MessageWire.sender_trust`:
    /// `"verified"` (DMARC pass), `"suspicious"` (an auth method
    /// failed — likely spoofed), `"unverified"`, or `""` for messages
    /// ingested before the signal existed. Self-hosted cryptographic
    /// auth result, no model.
    pub sender_trust: String,
    pub recipients: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<String>,
    pub subject: String,
    pub flags: u32,
    pub internal_date: i64,
    pub message_id: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<serde_json::Value>,
    pub category: String,
    pub risk_score: u8,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub ai_analyzed: bool,
    pub clean_text: Option<String>,
    pub new_content: Option<String>,
    pub importance_level: String,
    pub importance_score: f32,
    pub is_bulk_sender: bool,
    pub has_tracking_pixel: bool,
    pub requires_action: bool,
    pub sender_intent: String,
    pub action_deadline: Option<String>,
    /// Where `List-Unsubscribe` says unsubscribing goes, absent when
    /// the message carries no usable target. 42.6% of the mail in this
    /// mailbox has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsubscribe: Option<UnsubscribeWire>,
}

/// The unsubscribe targets a message advertises.
///
/// `one_click` means the sender accepts an RFC 8058 POST, which the
/// **server** performs — see `POST /api/mail/unsubscribe`. The URLs are
/// still sent to the client so it can offer the link when one-click is
/// not on the table, but a client must not post to them itself: these
/// URLs identify the subscriber, and fetching one from a phone hands
/// the sender the reader's address and network.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnsubscribeWire {
    pub one_click: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mailto: Vec<String>,
}

impl From<mailrs_clean::Unsubscribe> for UnsubscribeWire {
    fn from(u: mailrs_clean::Unsubscribe) -> Self {
        Self {
            one_click: u.one_click,
            http: u.http,
            mailto: u.mailto,
        }
    }
}

impl ThreadMessageResponse {
    fn from_wire_no_body(w: mailrs_core_api::method::message::MessageWire) -> Self {
        Self {
            uid: w.uid,
            // idempotent rfc2047 pass for pre-fix rows (see summary conv)
            sender: mailrs_rfc2047::decode(w.sender.as_bytes()).into_owned(),
            sender_trust: w.sender_trust.clone(),
            recipients: mailrs_rfc2047::decode(w.recipients.as_bytes()).into_owned(),
            cc: None,
            subject: w.subject,
            flags: w.flags,
            internal_date: w.internal_date,
            message_id: w.message_id,
            text_body: None,
            html_body: None,
            attachments: Vec::new(),
            category: String::from("inbox"),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: String::from("normal"),
            importance_score: 0.0,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: String::new(),
            action_deadline: None,
            unsubscribe: None,
        }
    }
}

/// Extract the `Cc:` header from the raw RFC 5322 source. Follows folded
/// continuation lines (leading whitespace = continuation of prior header).
/// Returns `None` when no Cc header is present or when it's empty.
pub(crate) fn extract_cc_header(data: &[u8]) -> Option<String> {
    extract_header(data, b"cc:")
}

/// One named header's unfolded value.
///
/// `name` includes the colon, which is what keeps `list-unsubscribe:`
/// from also matching `list-unsubscribe-post:` — two different headers
/// that this code reads separately and must not confuse.
pub(crate) fn extract_header(data: &[u8], name: &[u8]) -> Option<String> {
    // Header block ends at the first blank line (CRLF CRLF or LF LF).
    let head_end = memchr_bytes(data, b"\r\n\r\n")
        .or_else(|| memchr_bytes(data, b"\n\n"))
        .unwrap_or(data.len());
    let head = &data[..head_end];
    let mut lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    for l in lines.iter_mut() {
        if l.last() == Some(&b'\r') {
            *l = &l[..l.len() - 1];
        }
    }
    let n = name.len();
    let mut i = 0;
    while i < lines.len() {
        let ll = lines[i];
        if ll.len() < n {
            i += 1;
            continue;
        }
        if ll.get(..n).is_some_and(|s| s.eq_ignore_ascii_case(name)) {
            let mut val: Vec<u8> = ll[n..].trim_ascii_start().to_vec();
            let mut j = i + 1;
            while j < lines.len() {
                let cont = lines[j];
                if cont.first().is_some_and(|c| *c == b' ' || *c == b'\t') {
                    val.push(b' ');
                    val.extend_from_slice(cont.trim_ascii());
                    j += 1;
                } else {
                    break;
                }
            }
            let s = String::from_utf8_lossy(&val).trim().to_string();
            return if s.is_empty() { None } else { Some(s) };
        }
        i += 1;
    }
    None
}

/// Extract the `Date:` header epoch (UTC seconds) from raw RFC 5322
/// bytes. Used to override `wire.internal_date` at read-time when the
/// stored value looks stale — the fastcore self-heal used to parse
/// dates without a proper RFC 2822 parser (dropped timezones, missing
/// dates fell to 0 → 1970), so historic threads still carry those
/// bad epochs. Re-parsing at read time fixes the sort order in the UI
/// without a bulk re-heal pass. Same folding rules as `extract_cc_header`.
pub(crate) fn extract_date_header_epoch(data: &[u8]) -> Option<i64> {
    let head_end = memchr_bytes(data, b"\r\n\r\n")
        .or_else(|| memchr_bytes(data, b"\n\n"))
        .unwrap_or(data.len());
    let head = &data[..head_end];
    let mut lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    for l in lines.iter_mut() {
        if l.last() == Some(&b'\r') {
            *l = &l[..l.len() - 1];
        }
    }
    let mut i = 0;
    while i < lines.len() {
        let ll = lines[i];
        if ll
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case(b"date:"))
        {
            let mut val: Vec<u8> = ll[5..].trim_ascii_start().to_vec();
            let mut j = i + 1;
            while j < lines.len() {
                let cont = lines[j];
                if cont.first().is_some_and(|c| *c == b' ' || *c == b'\t') {
                    val.push(b' ');
                    val.extend_from_slice(cont.trim_ascii());
                    j += 1;
                } else {
                    break;
                }
            }
            let s = String::from_utf8_lossy(&val).trim().to_string();
            return parse_rfc2822_epoch(&s);
        }
        i += 1;
    }
    None
}

/// Same retry ladder as fastcore's `parse_rfc5322_date`. Local copy
/// keeps webapi self-contained (chrono is already a transitive dep).
pub(crate) fn parse_rfc2822_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    let no_comment = s.split(" (").next().unwrap_or(s).trim_end();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(no_comment) {
        return Some(dt.timestamp());
    }
    let no_dow = match no_comment.find(", ") {
        Some(idx) => no_comment[idx + 2..].trim_start(),
        None => no_comment,
    };
    chrono::DateTime::parse_from_rfc2822(no_dow)
        .ok()
        .map(|dt| dt.timestamp())
}

pub(crate) fn memchr_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Parse `data` (raw RFC-5322 bytes) into text/html body + attachments.
/// Same pipeline as monolith's `crate::message_util::parse_message`.
pub(crate) fn parse_body(data: &[u8]) -> (Option<String>, Option<String>, Vec<serde_json::Value>) {
    let root = mailrs_mime::parse(data);
    let mut text_body: Option<String> = None;
    let mut html_body: Option<String> = None;
    for part in root.walk() {
        let mt = part.content_type.mime_type();
        if text_body.is_none() && mt == "text/plain" {
            text_body = part.body_text();
        } else if html_body.is_none() && mt == "text/html" {
            html_body = part.body_text();
        }
        if text_body.is_some() && html_body.is_some() {
            break;
        }
    }
    if text_body.is_none() && html_body.is_none() && root.children.is_empty() {
        if root.content_type.type_ == "text" {
            text_body = root.body_text();
        } else {
            text_body = Some(String::from_utf8_lossy(data).into_owned());
        }
    }
    let text_body = text_body.or_else(|| {
        html_body
            .as_deref()
            .and_then(|html| html2text::from_read(html.as_bytes(), 80).ok())
    });
    let attachments: Vec<serde_json::Value> = root
        .attachments()
        .map(|att| {
            let filename = att.attachment_filename().unwrap_or("unnamed").to_string();
            let mt = att.content_type.mime_type();
            let mt = if mt.ends_with('/') || mt.starts_with('/') {
                "application/octet-stream".to_string()
            } else {
                mt
            };
            // v2.5.0 Phase 5 (RFC-B §5): Content-ID header maps a
            // `multipart/related` inline image to its `<img src="cid:..">`
            // reference in the HTML body. Emit it so the frontend
            // HtmlFrame can rewrite the cid: URIs into
            // `/api/mail/messages/{uid}/attachments/{index}/content`
            // and the browser fetches the image inline. Omitted when
            // the part has no Content-ID (regular non-inline
            // attachments).
            let mut obj = serde_json::json!({
                "filename": filename,
                "content_type": mt,
                "size": att.body.len() as u32,
            });
            if let Some(cid) = &att.content_id
                && let Some(map) = obj.as_object_mut()
            {
                map.insert("content_id".into(), serde_json::Value::String(cid.clone()));
            }
            obj
        })
        .collect();
    (text_body, html_body, attachments)
}

/// Public alias for the MCP handler to reuse the same body-enrichment
/// pipeline REST uses. Same signature; keeps `enrich_with_body`
/// module-private while giving MCP a stable seam.
pub async fn enrich_with_body_public(
    store: &dyn mailrs_message_store::MessageStore,
    maildir_root: &str,
    user: &str,
    w: mailrs_core_api::method::message::MessageWire,
) -> ThreadMessageResponse {
    enrich_with_body(store, maildir_root, user, w).await
}

/// Enrich a MessageWire with body content read from maildir.
///
/// Handles Maildir++ subfolders: fastcore's self-heal stores blob_ref
/// as `<subfolder>/<filename>` for files under `.Sent/`, `.Drafts/`,
/// etc. INBOX files stay as bare `<filename>`. We split on the first
/// `/` and append the subfolder segment to the maildir path so
/// `MaildirStore::fetch` opens the right sub-maildir.
pub(crate) async fn enrich_with_body(
    store: &dyn mailrs_message_store::MessageStore,
    maildir_root: &str,
    user: &str,
    w: mailrs_core_api::method::message::MessageWire,
) -> ThreadMessageResponse {
    let mut r = ThreadMessageResponse::from_wire_no_body(w.clone());
    if let Some((path, id)) =
        crate::handlers::messages::blob_ref_location(maildir_root, user, &w.blob_ref)
        && let Ok(Some(bytes)) = store.fetch(&path, &id).await
    {
        let (t, h, a) = parse_body(&bytes);
        r.text_body = t;
        r.html_body = h;
        r.attachments = a;
        r.cc = extract_cc_header(&bytes);
        r.unsubscribe = extract_header(&bytes, b"list-unsubscribe:")
            .and_then(|v| {
                let post = extract_header(&bytes, b"list-unsubscribe-post:");
                mailrs_clean::parse_unsubscribe(&v, post.as_deref())
            })
            .map(UnsubscribeWire::from);
        // Repair a stale internal_date at read time. Historic
        // messages had wire.internal_date = 0 (1970) whenever the
        // old fastcore date parser choked on the header — replace
        // with the freshly parsed epoch so the timeline sorts
        // right without needing a bulk re-heal. Only overrides
        // when we get a positive parse and the stored value is
        // clearly stale (<= 0 or older than the header epoch).
        if let Some(hdr_epoch) = extract_date_header_epoch(&bytes)
            && (r.internal_date <= 0 || r.internal_date > hdr_epoch + 86_400)
        {
            r.internal_date = hdr_epoch;
        }
    }
    r
}
