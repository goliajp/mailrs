//! Reading a message far enough to file it: headers, the sender-auth
//! verdict, the threading resolver, and the maildir filename conventions.
//!
//! Shared by the ingest path and the maildir sweep on purpose. A second
//! copy of `resolve_thread_by_ancestry` would thread the sweep's repairs
//! differently from the arrivals they repair, and the symptom of that is
//! two messages that should be one conversation and are not.

use std::sync::Arc;

use crate::{FastcoreState, parse_rfc5322_date, strip_angle};

/// Extract common headers from an RFC 5322 message. Returns
/// `(message_id, in_reply_to, references, subject, date_epoch, from, to)`.
///
/// `references` is every Message-ID token of the References header,
/// oldest (root) first. Threading resolves against the msgid→thread
/// index via `resolve_thread_by_ancestry`; `references[0]` is only the
/// last-resort root guess (it is NOT stable across hops — remote MUAs
/// rewrite it, which fragmented conversations before v2.9.5).
/// Read the sender-authentication verdict from a message's own
/// `Authentication-Results` header, folded to a stable token. Empty
/// when the header is absent (e.g. mail that reached the maildir by a
/// path that didn't stamp it). This is the self-hosted "is this sender
/// who they claim to be" signal — pure auth results, no model.
pub(crate) fn extract_sender_trust(raw: &[u8]) -> String {
    let head = &raw[..raw.len().min(16 * 1024)];
    // Find the (possibly folded) Authentication-Results field. Headers
    // are ASCII field names; scan lines, unfolding continuations.
    let text = String::from_utf8_lossy(head);
    let mut value: Option<String> = None;
    let mut collecting = false;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if collecting {
            if line.starts_with(' ') || line.starts_with('\t') {
                value.as_mut().unwrap().push(' ');
                value.as_mut().unwrap().push_str(line.trim());
                continue;
            }
            break; // header ended
        }
        if let Some(rest) = line
            .strip_prefix("Authentication-Results:")
            .or_else(|| line.strip_prefix("authentication-results:"))
        {
            value = Some(rest.trim().to_string());
            collecting = true;
        }
    }
    let Some(v) = value else {
        return String::new();
    };
    let results = mailrs_inbound::parse_auth_results(&v);
    if results.is_empty() {
        return String::new();
    }
    mailrs_inbound::sender_trust(&results).as_str().to_string()
}

pub(crate) fn extract_headers(
    raw: &[u8],
) -> (String, String, Vec<String>, String, i64, String, String) {
    let mut message_id = String::new();
    let mut in_reply_to = String::new();
    let mut references: Vec<String> = Vec::new();
    let mut subject = String::new();
    let mut date_epoch: i64 = 0;
    let mut from = String::new();
    let mut to = String::new();

    // We only need headers; stop at the first blank line.
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let head = &raw[..head_end];
    let s = String::from_utf8_lossy(head);
    // Unfold headers (RFC 5322 §2.2.3 — a header continues onto the
    // next line if that line starts with WSP).
    let mut cur = String::new();
    let mut lines: Vec<String> = Vec::new();
    for line in s.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            cur.push(' ');
            cur.push_str(line.trim_start());
        } else {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    for l in &lines {
        let Some((name, val)) = l.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match name.to_ascii_lowercase().as_str() {
            "message-id" => message_id = strip_angle(val),
            "in-reply-to" => in_reply_to = strip_angle(val),
            "references" => {
                // Every <...> token, oldest (root) first — the full chain
                // feeds the msgid→thread resolver, not just token 0.
                references = val
                    .split_whitespace()
                    .filter_map(|tok| {
                        let t = tok.trim_matches(|c: char| c == '<' || c == '>' || c == ',');
                        (!t.is_empty()).then(|| t.to_string())
                    })
                    .collect();
            }
            "subject" => subject = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            // display-name part of address headers is rfc2047-encoded by
            // many senders — decode here so stores never hold =?..?= runes
            "from" => from = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "to" => to = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "date" => date_epoch = parse_rfc5322_date(val).unwrap_or(0),
            _ => {}
        }
    }
    (
        message_id,
        in_reply_to,
        references,
        subject,
        date_epoch,
        from,
        to,
    )
}

/// Resolve which existing thread a message belongs to via the per-user
/// `Message-ID → thread_id` index. `None` = nothing known, caller falls
/// back to the legacy root rule. The message's OWN id is consulted
/// first — a message that was already ingested (and possibly moved by a
/// rethread merge) must land back in its current thread, or self-heal
/// re-creates the pre-merge fragment on every boot. Then nearest
/// ancestor wins: In-Reply-To, then References newest → oldest.
pub(crate) fn resolve_thread_by_ancestry(
    state: &Arc<FastcoreState>,
    user: &str,
    own_mid: &str,
    in_reply_to: &str,
    references: &[String],
    subject: &str,
) -> Option<String> {
    if !own_mid.is_empty()
        && let Ok(Some(tid)) = state.mailbox.thread_for_message_id(user, own_mid)
    {
        // own-id hits skip the subject gate: the message is already IN
        // that thread (re-ingest / self-heal), splitting it here would
        // fight the recorded state.
        return Some(tid);
    }
    let mut candidate: Option<String> = None;
    if !in_reply_to.is_empty()
        && let Ok(Some(tid)) = state.mailbox.thread_for_message_id(user, in_reply_to)
    {
        candidate = Some(tid);
    }
    if candidate.is_none() {
        for mid in references.iter().rev() {
            if let Ok(Some(tid)) = state.mailbox.thread_for_message_id(user, mid) {
                candidate = Some(tid);
                break;
            }
        }
    }
    // Gmail's subject rule: an ancestry match only joins the ancestor's
    // conversation when the normalized subjects agree. A reply that
    // changes topic ("annual closing" sent as a reply to the "withholding
    // tax" thread) is a NEW conversation — otherwise the old thread's
    // display flips to the user's own outbound subject and reads like a
    // sent mail sitting in the Inbox (2026-07-17 report).
    let tid = candidate?;
    let subj_norm = mailrs_mailbox_kevy::normalize_subject(subject);
    if subj_norm.is_empty() {
        return Some(tid);
    }
    match state.mailbox.get_thread(&tid) {
        Ok(Some(row)) => {
            if mailrs_mailbox_kevy::normalize_subject(&row.subject) == subj_norm {
                Some(tid)
            } else {
                None
            }
        }
        _ => Some(tid),
    }
}

/// Extract the delivery epoch from a Maildir filename. The Maildir
/// naming convention (`<epoch>.M<micro>P<pid>Q<seq>.<host>`) records
/// the delivery second in the leading component — a reliable fallback
/// when the message's `Date:` header is missing or unparseable. Filter
/// out obviously bogus epochs (<= year 2000) so we don't backdate
/// modern mail into 1970 territory.
pub(crate) fn maildir_filename_epoch(name: &str) -> Option<i64> {
    let first = name.split('.').next()?;
    let n: i64 = first.parse().ok()?;
    if n > 946_684_800 { Some(n) } else { None }
}

/// Whether a maildir filename carries the \Seen flag — the `:2,` info
/// section lists flags alphabetically (`...:2,RS` etc.).
pub(crate) fn maildir_seen_flag(name: &str) -> bool {
    match name.rsplit_once(":2,") {
        Some((_, info)) => info.contains('S'),
        None => false,
    }
}

/// The Maildir++ keyword bits in a file name's `:2,` suffix.
///
/// Lowercase letters, whose meaning is in the mailbox's `mailrs-keywords`
/// file — `archived` and `pinned` are written there because no standard
/// flag means either and because a person's decision cannot be recomputed
/// from the mail.
pub(crate) fn maildir_keyword_bits(name: &str) -> Vec<char> {
    match name.rsplit_once(":2,") {
        Some((_, info)) => {
            let mut out: Vec<char> = info.chars().filter(|c| c.is_ascii_lowercase()).collect();
            out.sort_unstable();
            out.dedup();
            out
        }
        None => Vec::new(),
    }
}

/// Fall back to the file's mtime as the delivery epoch when both the
/// `Date:` header and the maildir filename yield nothing usable.
pub(crate) fn file_mtime_epoch(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Extract the searchable text of a message: the `text/plain` part if
/// there is one, else the `text/html` part flattened. Returns `None`
/// when neither exists (a bare attachment, say) so the caller can skip
/// writing an empty row.
pub(crate) fn body_text_for_search(raw: &[u8]) -> Option<String> {
    let root = mailrs_mime::parse(raw);
    let mut html: Option<String> = None;
    for part in root.walk() {
        match part.content_type.mime_type().as_str() {
            "text/plain" => {
                if let Some(t) = part.body_text() {
                    return Some(t);
                }
            }
            "text/html" if html.is_none() => html = part.body_text(),
            _ => {}
        }
    }
    if let Some(h) = html {
        return Some(html2text::from_read(h.as_bytes(), 100).unwrap_or(h));
    }
    root.body_text()
}
