//! Mailbox-level commands: LIST, SELECT, CLOSE, APPEND, IDLE.
//!
//! IDLE reads the kevy change feed rather than a broadcast channel, so a
//! client stays caught up across a fastcore restart —
//! `.claude/rules/kevy-patterns.md` → `kevy/change-stream`.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{
    format_bad, format_exists, format_flags, format_list, format_no, format_ok, format_recent,
    special_use_flag,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::backend::{self, MailboxInfo};
use crate::FastcoreState;

use super::fetch::*;
use super::session::State;

pub(super) fn list_response(
    state: &Arc<FastcoreState>,
    session: &State,
    tag: &str,
    pattern: &str,
) -> Vec<String> {
    let Some(user) = session.user() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let mailboxes = backend::list_mailboxes(state, user);
    let mut out = Vec::with_capacity(mailboxes.len() + 1);
    for mb in mailboxes {
        if match_wildcard(&mb.name, pattern) {
            // v2.4.2 Phase 4 (RFC-C, RFC 6154 §5.1): concatenate the
            // SPECIAL-USE tag after `\HasNoChildren` so MUAs
            // (Thunderbird, Apple Mail, iOS Mail) auto-map the
            // recognized folder names to their built-in Junk / Sent
            // / Drafts / Trash / Archive UIs. Empty string when the
            // mailbox name isn't one of the recognized leaves.
            let flags = format!("\\HasNoChildren{}", special_use_flag(&mb.name));
            out.push(format_list(&flags, "/", &mb.name));
        }
    }
    out.push(format_ok(tag, "LIST completed"));
    out
}

/// IMAP wildcard match — `*` recursive, `%` single-level (RFC 3501
/// §6.3.8 says `%` is one hierarchy level but our maildir has no
/// hierarchy separator so `%` and `*` collapse). An empty pattern
/// matches everything, matching most clients' initial LIST probe.
pub(super) fn match_wildcard(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() || pattern == "*" || pattern == "%" {
        return true;
    }
    // Recursive glob match: split on `*` / `%`, require each literal
    // segment to appear in order.
    let mut segments: Vec<&str> = Vec::new();
    let mut cur = 0;
    let bytes = pattern.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'*' || *b == b'%' {
            if i > cur {
                segments.push(&pattern[cur..i]);
            }
            segments.push("*");
            cur = i + 1;
        }
    }
    if cur < bytes.len() {
        segments.push(&pattern[cur..]);
    }
    let name_lower = name.to_lowercase();
    let mut pos = 0;
    let mut requires_prefix = !pattern.starts_with('*') && !pattern.starts_with('%');
    let requires_suffix = !pattern.ends_with('*') && !pattern.ends_with('%');
    let last_idx = segments.len().saturating_sub(1);
    for (i, seg) in segments.iter().enumerate() {
        if *seg == "*" {
            continue;
        }
        let seg_l = seg.to_lowercase();
        let search_from = pos;
        let found = if i == 0 && requires_prefix {
            if name_lower.starts_with(&seg_l) {
                Some(0)
            } else {
                None
            }
        } else {
            name_lower[search_from..]
                .find(&seg_l)
                .map(|p| p + search_from)
        };
        let Some(f) = found else { return false };
        pos = f + seg_l.len();
        requires_prefix = false;
        if i == last_idx && requires_suffix && pos != name_lower.len() {
            return false;
        }
    }
    true
}

pub(super) fn select(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    mailbox: &str,
    read_only: bool,
    qresync: bool,
) -> Vec<String> {
    let Some(user) = session.user().map(str::to_string) else {
        return vec![format_no(tag, "not authenticated")];
    };
    // `SELECT "INBOX" (QRESYNC (uidvalidity modseq))` — the proto hands
    // the whole tail as the mailbox arg; split the optional QRESYNC
    // parameter list off (RFC 7162 §3.2.5)
    let (mailbox, qresync_params) = match mailbox.split_once(" (") {
        Some((name, params)) => {
            let name = name.trim().trim_matches('"');
            let up = params.to_ascii_uppercase();
            let parsed = up.strip_prefix("QRESYNC (").and_then(|rest| {
                let rest = rest.trim_end_matches(')');
                let mut it = rest.split_whitespace();
                let uv = it.next()?.parse::<u32>().ok()?;
                let ms = it.next()?.parse::<u64>().ok()?;
                Some((uv, ms))
            });
            (name, parsed)
        }
        None => (mailbox, None),
    };
    let Some(mb) = backend::get_mailbox(state, &user, mailbox) else {
        return vec![format_no(tag, "no such mailbox")];
    };
    let messages = backend::list_messages(state, &user, &mb);
    let count = messages.len() as u32;
    let recent = count; // We don't distinguish; every scan is fresh.
    let uidnext = backend::uid_next(state, &user);
    let uidvalidity = backend::uidvalidity(state, &user, mailbox);
    let highestmodseq = backend::highest_modseq(state, &user);
    let flags_line = format_flags(&["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"]);
    let permanent = if read_only {
        "* OK [PERMANENTFLAGS ()] Read-only\r\n".to_string()
    } else {
        "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)] Limited\r\n"
            .to_string()
    };
    let mut out = vec![
        flags_line,
        format_exists(count),
        format_recent(recent),
        format!("* OK [UIDVALIDITY {uidvalidity}] Version 1\r\n"),
        format!("* OK [UIDNEXT {uidnext}] Predicted next UID\r\n"),
        format!("* OK [HIGHESTMODSEQ {highestmodseq}] Modseq\r\n"),
        permanent,
        format_ok(
            tag,
            if read_only {
                "[READ-ONLY] EXAMINE completed"
            } else {
                "[READ-WRITE] SELECT completed"
            },
        ),
    ];
    // QRESYNC delta: only when the client ENABLEd it, supplied params,
    // and its cached uidvalidity still matches
    let mut qresync_lines: Vec<String> = Vec::new();
    if qresync
        && let Some((client_uv, client_ms)) = qresync_params
        && client_uv == uidvalidity
    {
        let vanished = backend::vanished_since(state, &user, mailbox, client_ms);
        if !vanished.is_empty() {
            let list = vanished
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            qresync_lines.push(format!("* VANISHED (EARLIER) {list}\r\n"));
        }
        for m in &messages {
            if m.modseq > client_ms {
                let flags_str = flags_to_imap(&m.flags);
                qresync_lines.push(format!(
                    "* {} FETCH (UID {} FLAGS ({flags_str}) MODSEQ ({}))\r\n",
                    m.seqno, m.uid, m.modseq
                ));
            }
        }
    }
    *session = State::Selected {
        user,
        mailbox: mb,
        messages,
        read_only,
    };
    // untagged QRESYNC deltas go before the tagged completion
    if !qresync_lines.is_empty()
        && let Some(tagged) = out.pop()
    {
        out.extend(qresync_lines);
        out.push(tagged);
    }
    out
}

pub(super) fn close(session: &mut State, tag: &str) -> Vec<String> {
    let State::Selected { user, .. } = session.clone() else {
        return vec![format_bad(tag, "not in SELECTED state")];
    };
    *session = State::Authed { user };
    vec![format_ok(tag, "CLOSE completed")]
}

pub(super) async fn append_flow<S>(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    framed: &mut Framed<S, ImapCodec>,
    mailbox: &str,
    literal_size: u32,
) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let user = match session.user() {
        Some(u) => u.to_string(),
        None => return vec![format_no(tag, "not authenticated")],
    };
    let Some(dest) = backend::get_mailbox(state, &user, mailbox) else {
        return vec![format_no(tag, "no such mailbox")];
    };
    // Ask codec to switch to literal mode + prompt the client for the
    // payload.
    framed.codec_mut().expect_literal(literal_size);
    if framed
        .send(b"+ Ready for literal data\r\n".to_vec())
        .await
        .is_err()
    {
        return vec![format_no(tag, "network error")];
    }
    let bytes = match framed.next().await {
        Some(Ok(ImapInput::LiteralData(bytes))) => bytes,
        _ => return vec![format_no(tag, "expected literal data")],
    };
    let user = user.to_string();
    match backend::append(state, &user, &dest, &bytes) {
        Ok(_uid) => vec![format_ok(tag, "APPEND completed")],
        Err(e) => vec![format_no(tag, &format!("append failed: {e}"))],
    }
}

/// RFC 2177 IDLE with real push. Subscribes to the in-process delivery
/// broadcast; on an event for this user, rescans the selected mailbox
/// and emits `* n EXISTS` (+ RECENT) when the count grew. Ends when the
/// client sends DONE, the connection drops, or the 29-minute inactivity
/// ceiling passes (clients re-issue IDLE well before that per the RFC).
pub(super) async fn idle_flow<S>(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    framed: &mut Framed<S, ImapCodec>,
) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let (user, mailbox) = match &session {
        State::Selected { user, mailbox, .. } => (user.clone(), mailbox.clone()),
        State::Authed { .. } => {
            // legal per RFC but there is no mailbox to report on — hold
            // the line open without events
            (
                String::new(),
                MailboxInfo {
                    name: String::new(),
                    path: std::path::PathBuf::new(),
                },
            )
        }
        _ => return vec![format_no(tag, "not authenticated")],
    };
    if framed.send(b"+ idling\r\n".to_vec()).await.is_err() {
        return Vec::new();
    }
    // v2 Stage B.8: subscribe to the kevy 3.17 change feed instead of
    // the tokio broadcast::channel. The feed is durable — a fastcore
    // restart resumes from the last delivered offset, so events that
    // fire mid-restart are not lost (broadcast::channel is in-memory
    // and drops on restart). Consumer polls at ~500 ms cadence, well
    // within the RFC 2177 IDLE spec's "poll frequently enough that
    // clients see mail within a few seconds" guidance.
    let (mut feed_gen, mut feed_off) = state.mailbox.store_ref().changes_tail().unwrap_or((0, 0));
    let user_prefix = format!("mailrs:user:{user}:");
    let mut known = match &session {
        State::Selected { messages, .. } => messages.len(),
        _ => 0,
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(29 * 60);
    let mut change_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    change_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            frame = framed.next() => {
                match frame {
                    Some(Ok(ImapInput::Line(l))) if l.trim().eq_ignore_ascii_case("DONE") => {
                        break;
                    }
                    Some(Ok(_)) => continue, // anything else mid-IDLE: ignore
                    _ => return Vec::new(),  // connection gone
                }
            }
            _ = change_tick.tick() => {
                let batch = state
                    .mailbox
                    .store_ref()
                    .changes_since(feed_gen, feed_off, 100, &[user_prefix.as_bytes()]);
                let Ok(batch) = batch else { continue }; // Disabled | Resync — silently skip; a re-select recovers
                if batch.changes.is_empty() {
                    (feed_gen, feed_off) = batch.next;
                    continue;
                }
                (feed_gen, feed_off) = batch.next;
                let fresh = backend::list_messages(state, &user, &mailbox);
                if fresh.len() > known {
                    known = fresh.len();
                    let exists = format_exists(known as u32);
                    if framed.send(exists.into_bytes()).await.is_err() {
                        return Vec::new();
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                // inactivity ceiling — terminate the command; a live
                // client re-issues IDLE immediately
                break;
            }
        }
    }
    // refresh the session view so post-IDLE FETCHes see the new mail
    if let State::Selected {
        user,
        mailbox,
        messages,
        ..
    } = session
    {
        *messages = backend::list_messages(state, user, mailbox);
    }
    vec![format_ok(tag, "IDLE terminated")]
}
