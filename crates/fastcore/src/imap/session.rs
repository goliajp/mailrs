//! Per-connection IMAP session — state machine + command dispatch.
//!
//! Structure follows RFC 3501 §3: NotAuthenticated → Authenticated →
//! Selected → Logout. LOGIN flips NotAuthenticated → Authenticated;
//! SELECT / EXAMINE flip Authenticated → Selected; LOGOUT / CLOSE
//! terminate the connection. Unknown / unsupported commands answer
//! `BAD`, unauthenticated commands answer `NO`.
//!
//! The session leans heavily on `mailrs-imap-proto` (parser +
//! formatter) and `mailrs-imap-codec` (framing) so we don't reinvent
//! the wire format. Backend calls go through [`crate::imap::backend`].

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{
    ImapCommand, format_bad, format_bye, format_capability, format_exists, format_fetch,
    format_flags, format_list, format_no, format_ok, format_recent, parse_command,
};
use mailrs_maildir::Flag;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::backend::{self, ImapMessage, MailboxInfo};
use crate::FastcoreState;

/// Session state per RFC 3501 §3.
#[derive(Debug, Clone)]
enum State {
    NotAuthed,
    Authed {
        user: String,
    },
    Selected {
        user: String,
        mailbox: MailboxInfo,
        messages: Vec<ImapMessage>,
        read_only: bool,
    },
}

impl State {
    fn user(&self) -> Option<&str> {
        match self {
            State::NotAuthed => None,
            State::Authed { user } | State::Selected { user, .. } => Some(user),
        }
    }
}

/// Entry point — takes a plaintext connection and drives it to
/// completion. STARTTLS transitions happen at the listener layer
/// (this loop doesn't own the socket type after upgrade).
pub async fn run<S>(state: Arc<FastcoreState>, io: S)
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut framed = Framed::new(io, ImapCodec::new());
    // Greeting per RFC 3501 §7.5. Sent before the client says anything.
    let greeting = format!(
        "* OK [CAPABILITY IMAP4rev1 IDLE STARTTLS AUTH=PLAIN NAMESPACE ENABLE SORT QUOTA CONDSTORE QRESYNC] {} ready\r\n",
        state
            .mailbox
            .store_ref()
            .get(b"hostname")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| "mailrs".into())
    );
    if framed.send(greeting.into_bytes()).await.is_err() {
        return;
    }

    let mut session = State::NotAuthed;
    // ENABLE QRESYNC is connection-scoped (RFC 7162 §3.2.5)
    let mut qresync = false;
    while let Some(frame) = framed.next().await {
        let Ok(input) = frame else { return };
        let line = match input {
            ImapInput::Line(s) => s,
            ImapInput::LiteralData(_) => {
                // Standalone literal outside APPEND flow — ignore.
                continue;
            }
        };
        let parsed = match parse_command(line.trim_end()) {
            Ok(cmd) => cmd,
            Err(e) => {
                let _ = framed
                    .send(format_bad("*", &format!("parse: {e}")).into_bytes())
                    .await;
                continue;
            }
        };
        let is_logout = matches!(parsed.command, ImapCommand::Logout);
        let tag = parsed.tag;
        let responses = dispatch(
            &state,
            &mut session,
            &tag,
            parsed.command,
            &mut framed,
            &mut qresync,
        )
        .await;
        for r in responses {
            if framed.send(r.into_bytes()).await.is_err() {
                return;
            }
        }
        if is_logout {
            return;
        }
    }
}

async fn dispatch<S>(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    cmd: ImapCommand,
    framed: &mut Framed<S, ImapCodec>,
    qresync: &mut bool,
) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    match cmd {
        ImapCommand::Capability => vec![
            format_capability(&[
                "IMAP4rev1",
                "AUTH=PLAIN",
                "IDLE",
                "NAMESPACE",
                "ENABLE",
                "UNSELECT",
                "MOVE",
                "SPECIAL-USE",
                "SORT",
                "QUOTA",
                "CONDSTORE",
                "QRESYNC",
            ]),
            format_ok(tag, "CAPABILITY completed"),
        ],
        ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
        ImapCommand::Logout => vec![
            format_bye("mailrs logging out"),
            format_ok(tag, "LOGOUT completed"),
        ],
        ImapCommand::Login { username, password } => {
            login(state, session, tag, &username, &password)
        }
        ImapCommand::List {
            reference: _,
            pattern,
        } => list_response(state, session, tag, &pattern),
        ImapCommand::Select { mailbox } => select(state, session, tag, &mailbox, false, *qresync),
        ImapCommand::Examine { mailbox } => select(state, session, tag, &mailbox, true, *qresync),
        ImapCommand::Enable(caps) => {
            let mut enabled = Vec::new();
            for c in &caps {
                let up = c.to_ascii_uppercase();
                if up == "QRESYNC" || up == "CONDSTORE" {
                    if up == "QRESYNC" {
                        *qresync = true;
                    }
                    enabled.push(up);
                }
            }
            vec![
                format!("* ENABLED {}\r\n", enabled.join(" ")),
                format_ok(tag, "ENABLE completed"),
            ]
        }
        ImapCommand::Close => close(session, tag),
        ImapCommand::Fetch {
            sequence,
            attributes,
        } => fetch_response(session, tag, &sequence, &attributes, false),
        ImapCommand::Uid { subcommand } => match *subcommand {
            ImapCommand::Fetch {
                sequence,
                attributes,
            } => fetch_response(session, tag, &sequence, &attributes, true),
            ImapCommand::Store {
                sequence,
                action,
                flags,
            } => store_response(state, session, tag, &sequence, &action, &flags, true),
            ImapCommand::Search { criteria } => search_response(session, tag, &criteria, true),
            ImapCommand::Sort {
                criteria,
                charset: _,
                search_criteria,
            } => sort_response(session, tag, &criteria, &search_criteria, true),
            ImapCommand::Copy { sequence, mailbox } => {
                copy_response(state, session, tag, &sequence, &mailbox, false, true)
            }
            ImapCommand::Move { sequence, mailbox } => {
                copy_response(state, session, tag, &sequence, &mailbox, true, true)
            }
            _ => vec![format_bad(tag, "UID subcommand not supported")],
        },
        ImapCommand::Store {
            sequence,
            action,
            flags,
        } => store_response(state, session, tag, &sequence, &action, &flags, false),
        ImapCommand::Search { criteria } => search_response(session, tag, &criteria, false),
        ImapCommand::Expunge => expunge(state, session, tag, *qresync),
        ImapCommand::Copy { sequence, mailbox } => {
            copy_response(state, session, tag, &sequence, &mailbox, false, false)
        }
        ImapCommand::Move { sequence, mailbox } => {
            copy_response(state, session, tag, &sequence, &mailbox, true, false)
        }
        ImapCommand::Append {
            mailbox,
            flags: _flags,
            literal_size,
        } => append_flow(state, session, tag, framed, &mailbox, literal_size).await,
        ImapCommand::Idle => idle_flow(state, session, tag, framed).await,
        ImapCommand::Sort {
            criteria,
            charset: _,
            search_criteria,
        } => sort_response(session, tag, &criteria, &search_criteria, false),
        ImapCommand::GetQuota { quotaroot: _ } => quota_response(state, session, tag),
        ImapCommand::GetQuotaRoot { mailbox } => {
            let mut out = vec![format!("* QUOTAROOT \"{mailbox}\" \"\"\r\n")];
            out.extend(quota_response(state, session, tag));
            out
        }
        _ => vec![format_bad(tag, "command not implemented")],
    }
}

fn login(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    username: &str,
    password: &str,
) -> Vec<String> {
    if !matches!(session, State::NotAuthed) {
        return vec![format_bad(tag, "already authenticated")];
    }
    if backend::verify_password(state, username, password) {
        *session = State::Authed {
            user: username.to_string(),
        };
        vec![format_ok(tag, "LOGIN completed")]
    } else {
        vec![format_no(tag, "invalid credentials")]
    }
}

fn list_response(
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
            out.push(format_list("\\HasNoChildren", "/", &mb.name));
        }
    }
    out.push(format_ok(tag, "LIST completed"));
    out
}

/// IMAP wildcard match — `*` recursive, `%` single-level (RFC 3501
/// §6.3.8 says `%` is one hierarchy level but our maildir has no
/// hierarchy separator so `%` and `*` collapse). An empty pattern
/// matches everything, matching most clients' initial LIST probe.
fn match_wildcard(name: &str, pattern: &str) -> bool {
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

fn select(
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

fn close(session: &mut State, tag: &str) -> Vec<String> {
    let State::Selected { user, .. } = session.clone() else {
        return vec![format_bad(tag, "not in SELECTED state")];
    };
    *session = State::Authed { user };
    vec![format_ok(tag, "CLOSE completed")]
}

fn fetch_response(
    session: &State,
    tag: &str,
    sequence: &str,
    attributes: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected { messages, .. } = session else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    let ids = expand_sequence(sequence, messages, by_uid);
    // CHANGEDSINCE modifier (RFC 7162): `FETCH 1:* (FLAGS) (CHANGEDSINCE 42)`
    let attrs_upper = attributes.to_uppercase();
    let changedsince = attrs_upper.find("CHANGEDSINCE").and_then(|pos| {
        attributes
            .get(pos + "CHANGEDSINCE".len()..)
            .unwrap_or("")
            .split_whitespace()
            .next()
            .and_then(|t| t.trim_end_matches(')').parse::<u64>().ok())
    });
    let mut out = Vec::with_capacity(ids.len() + 1);
    for msg in ids {
        if let Some(since) = changedsince
            && msg.modseq <= since
        {
            continue;
        }
        let mut items = fetch_items(&msg, attributes, by_uid);
        // MODSEQ item: explicit request or implied by CHANGEDSINCE
        if changedsince.is_some() || attrs_upper.contains("MODSEQ") {
            items.push(("MODSEQ".into(), format!("({})", msg.modseq)));
        }
        out.push(format_fetch(msg.seqno, &items));
    }
    out.push(format_ok(tag, "FETCH completed"));
    out
}

fn fetch_items(msg: &ImapMessage, attrs: &str, by_uid: bool) -> Vec<(String, String)> {
    let mut items = Vec::new();
    let upper = attrs.to_uppercase();
    if by_uid || upper.contains("UID") {
        items.push(("UID".into(), msg.uid.to_string()));
    }
    if upper.contains("FLAGS") {
        let flags_str = flags_to_imap(&msg.flags);
        items.push(("FLAGS".into(), format!("({flags_str})")));
    }
    if upper.contains("INTERNALDATE") {
        items.push((
            "INTERNALDATE".into(),
            format!("\"{}\"", format_internal_date(msg.internal_date)),
        ));
    }
    if upper.contains("RFC822.SIZE") || upper.contains("SIZE") {
        items.push(("RFC822.SIZE".into(), msg.size.to_string()));
    }
    if upper.contains("BODY[HEADER]") || upper.contains("RFC822.HEADER") {
        if let Some(bytes) = backend::read_message(msg) {
            let head_end = memmem(&bytes, b"\r\n\r\n")
                .or_else(|| memmem(&bytes, b"\n\n"))
                .unwrap_or(bytes.len());
            let head = &bytes[..head_end];
            let s = String::from_utf8_lossy(head).to_string();
            items.push(("BODY[HEADER]".into(), format!("{{{}}}\r\n{}", s.len(), s)));
        }
    } else if (upper.contains("BODY[]") || upper.contains("RFC822"))
        && let Some(bytes) = backend::read_message(msg)
    {
        items.push((
            "BODY[]".into(),
            format!("{{{}}}\r\n{}", bytes.len(), String::from_utf8_lossy(&bytes)),
        ));
    }
    items
}

fn flags_to_imap(flags: &[Flag]) -> String {
    flags
        .iter()
        .map(|f| match f {
            Flag::Seen => "\\Seen",
            Flag::Replied => "\\Answered",
            Flag::Flagged => "\\Flagged",
            Flag::Trashed => "\\Deleted",
            Flag::Draft => "\\Draft",
            Flag::Passed => "\\Answered",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_imap_flags(s: &str) -> Vec<Flag> {
    let cleaned = s.trim_matches(|c| c == '(' || c == ')');
    cleaned
        .split_whitespace()
        .filter_map(|w| match w.to_uppercase().as_str() {
            "\\SEEN" => Some(Flag::Seen),
            "\\ANSWERED" => Some(Flag::Replied),
            "\\FLAGGED" => Some(Flag::Flagged),
            "\\DELETED" => Some(Flag::Trashed),
            "\\DRAFT" => Some(Flag::Draft),
            _ => None,
        })
        .collect()
}

fn format_internal_date(epoch: i64) -> String {
    use chrono::{DateTime, Utc};
    match DateTime::<Utc>::from_timestamp(epoch, 0) {
        Some(dt) => dt.format("%d-%b-%Y %H:%M:%S +0000").to_string(),
        None => "01-Jan-1970 00:00:00 +0000".to_string(),
    }
}

fn store_response(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    sequence: &str,
    action: &str,
    flags: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected {
        user,
        messages,
        read_only,
        ..
    } = session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    if *read_only {
        return vec![format_no(tag, "mailbox is read-only")];
    }
    let user_owned = user.clone();
    let ids = expand_sequence(sequence, messages, by_uid);
    // UNCHANGEDSINCE modifier (RFC 7162). Proto splits STORE args as
    // (sequence, action, flags); with the modifier present the pieces
    // arrive as action="(UNCHANGEDSINCE", flags="<n>) +FLAGS (...)" —
    // same reassembly the monolith session used.
    let (unchangedsince, action_owned, flags_owned) =
        if action.to_ascii_uppercase().starts_with("(UNCHANGEDSINCE") {
            match flags.split_once(')') {
                Some((n, rest)) => {
                    let modseq = n.trim().parse::<u64>().ok();
                    let rest = rest.trim();
                    match rest.split_once(' ') {
                        Some((act, flg)) => (modseq, act.to_string(), flg.to_string()),
                        None => (modseq, rest.to_string(), String::new()),
                    }
                }
                None => (None, action.to_string(), flags.to_string()),
            }
        } else {
            (None, action.to_string(), flags.to_string())
        };
    let new_flags = parse_imap_flags(&flags_owned);
    let action_upper = action_owned.to_uppercase();
    let mut modified: Vec<u32> = Vec::new();
    let mut out = Vec::with_capacity(ids.len() + 1);
    for msg in ids {
        if let Some(since) = unchangedsince
            && msg.modseq > since
        {
            // changed behind the client's back — refuse this one
            modified.push(if by_uid { msg.uid } else { msg.seqno });
            continue;
        }
        let mut merged: Vec<Flag> = match action_upper.as_str() {
            a if a.starts_with("+FLAGS") => {
                let mut m = msg.flags.clone();
                for f in &new_flags {
                    if !m.contains(f) {
                        m.push(*f);
                    }
                }
                m
            }
            a if a.starts_with("-FLAGS") => msg
                .flags
                .iter()
                .copied()
                .filter(|f| !new_flags.contains(f))
                .collect(),
            _ => new_flags.clone(),
        };
        merged.sort_by_key(|f| *f as u32);
        merged.dedup();
        if backend::set_flags(&msg, &merged).is_ok() {
            let m = backend::bump_modseq(state, &user_owned);
            if let Some(base) = msg
                .path
                .file_name()
                .and_then(|f| f.to_str())
                .and_then(|f| f.split(':').next())
            {
                backend::set_file_modseq(state, &user_owned, base, m);
            }
        }
        if !action_upper.ends_with(".SILENT") {
            let flags_str = flags_to_imap(&merged);
            out.push(format_fetch(
                msg.seqno,
                &[("FLAGS".into(), format!("({flags_str})"))],
            ));
        }
    }
    // Refresh session view since paths changed.
    if let State::Selected {
        user,
        mailbox,
        messages,
        ..
    } = session
    {
        *messages = backend::list_messages(state, user, mailbox);
    }
    if modified.is_empty() {
        out.push(format_ok(tag, "STORE completed"));
    } else {
        let list = modified
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!(
            "{tag} OK [MODIFIED {list}] Conditional STORE failed for some messages\r\n"
        ));
    }
    out
}

fn search_response(session: &State, tag: &str, criteria: &str, by_uid: bool) -> Vec<String> {
    let State::Selected { messages, .. } = session else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    // full RFC 3501 grammar: implicit AND, OR / NOT, parenthesized
    // groups, HEADER / LARGER / SMALLER, dates, UID sets (G3.5)
    let keys = mailrs_imap_proto::parse_search_criteria(criteria);
    let matches: Vec<u32> = super::search_eval::filter(&keys, messages)
        .into_iter()
        .map(|m| if by_uid { m.uid } else { m.seqno })
        .collect();
    let list = matches
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let untagged = if list.is_empty() {
        "* SEARCH\r\n".to_string()
    } else {
        format!("* SEARCH {list}\r\n")
    };
    vec![untagged, format_ok(tag, "SEARCH completed")]
}

/// `SORT (criteria) charset search-keys` (RFC 5256) — filter via the
/// same evaluator SEARCH uses, then order per criteria (G3.4).
fn sort_response(
    session: &State,
    tag: &str,
    criteria: &str,
    search: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected { messages, .. } = session else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    let keys = mailrs_imap_proto::parse_search_criteria(search);
    let matched = super::search_eval::filter(&keys, messages);
    let (reverse, crits) = super::search_eval::parse_sort_criteria(criteria);
    let sorted = super::search_eval::sort(matched, reverse, &crits);
    let list = sorted
        .iter()
        .map(|m| if by_uid { m.uid } else { m.seqno })
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let untagged = if list.is_empty() {
        "* SORT\r\n".to_string()
    } else {
        format!("* SORT {list}\r\n")
    };
    vec![untagged, format_ok(tag, "SORT completed")]
}

/// `GETQUOTA` / the quota half of `GETQUOTAROOT` (RFC 2087). Reads the
/// same network-kevy counters the receiver's 452 gate uses (G3.3 / G7).
/// STORAGE units are KiB per the RFC. No limit configured → empty
/// resource list (unlimited).
fn quota_response(_state: &Arc<FastcoreState>, session: &State, tag: &str) -> Vec<String> {
    let Some(user) = session.user() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let (limit, used) = crate::live_sync::quota_read(user);
    let line = if limit > 0 {
        format!(
            "* QUOTA \"\" (STORAGE {} {})\r\n",
            used / 1024,
            limit / 1024
        )
    } else {
        "* QUOTA \"\" ()\r\n".to_string()
    };
    vec![line, format_ok(tag, "GETQUOTA completed")]
}

fn expunge(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    qresync: bool,
) -> Vec<String> {
    let State::Selected {
        user,
        messages,
        read_only,
        mailbox,
        ..
    } = session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    if *read_only {
        return vec![format_no(tag, "mailbox is read-only")];
    }
    let mut out = Vec::new();
    // Iterate high → low so seqnos we emit are correct — RFC 3501
    // requires * EXPUNGE for each deleted message in descending order.
    let mut to_delete: Vec<u32> = messages
        .iter()
        .filter(|m| m.flags.contains(&Flag::Trashed))
        .map(|m| m.seqno)
        .collect();
    to_delete.sort_unstable_by(|a, b| b.cmp(a));
    let mut vanished_uids: Vec<u32> = Vec::new();
    for seqno in &to_delete {
        if let Some(m) = messages.iter().find(|m| m.seqno == *seqno) {
            if backend::delete_file(m).is_ok() {
                crate::live_sync::adjust_usage_bytes(user, -(m.size as i64));
                let ms = backend::bump_modseq(state, user);
                backend::record_vanished(state, user, &mailbox.name, m.uid, ms);
                vanished_uids.push(m.uid);
            }
            if !qresync {
                out.push(format!("* {seqno} EXPUNGE\r\n"));
            }
        }
    }
    // QRESYNC-enabled sessions get VANISHED instead of seqno EXPUNGE
    if qresync && !vanished_uids.is_empty() {
        vanished_uids.sort_unstable();
        let list = vanished_uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!("* VANISHED {list}\r\n"));
    }
    *messages = backend::list_messages(state, user, mailbox);
    out.push(format_ok(tag, "EXPUNGE completed"));
    out
}

fn copy_response(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    sequence: &str,
    mailbox: &str,
    move_op: bool,
    by_uid: bool,
) -> Vec<String> {
    let user = match session.clone() {
        State::Selected { user, .. } => user,
        _ => return vec![format_no(tag, "not in SELECTED state")],
    };
    let Some(dest) = backend::get_mailbox(state, &user, mailbox) else {
        return vec![format_no(tag, "no such destination")];
    };
    let State::Selected {
        messages,
        mailbox: src_mb,
        ..
    } = session
    else {
        unreachable!("checked above");
    };
    let ids = expand_sequence(sequence, messages, by_uid);
    for msg in &ids {
        if backend::copy_to(msg, &dest).is_err() {
            return vec![format_no(tag, "copy failed")];
        }
        if move_op {
            let _ = backend::delete_file(msg);
        } else {
            // COPY duplicates the bytes under the same account
            crate::live_sync::adjust_usage_bytes(&user, msg.size as i64);
        }
    }
    // Refresh source mailbox view.
    *messages = backend::list_messages(state, &user, src_mb);
    vec![format_ok(
        tag,
        if move_op {
            "MOVE completed"
        } else {
            "COPY completed"
        },
    )]
}

async fn append_flow<S>(
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
async fn idle_flow<S>(
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
    let mut rx = state.notify.subscribe();
    let mut known = match &session {
        State::Selected { messages, .. } => messages.len(),
        _ => 0,
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(29 * 60);
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
            evt = rx.recv() => {
                let Ok(delivered_user) = evt else { continue }; // lagged — resync below anyway
                if user.is_empty() || !delivered_user.eq_ignore_ascii_case(&user) {
                    continue;
                }
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

/// Expand an IMAP sequence set (`1:5`, `*`, `2,5,8:10`) to matching
/// messages. Uses seqno when `by_uid` is false, UID otherwise.
fn expand_sequence(spec: &str, messages: &[ImapMessage], by_uid: bool) -> Vec<ImapMessage> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let (lo, hi) = if let Some((l, h)) = part.split_once(':') {
            let lo = parse_seq_bound(l, messages, by_uid);
            let hi = parse_seq_bound(h, messages, by_uid);
            (lo, hi)
        } else {
            let n = parse_seq_bound(part, messages, by_uid);
            (n, n)
        };
        for m in messages {
            let cmp = if by_uid { m.uid } else { m.seqno };
            if cmp >= lo && cmp <= hi {
                out.push(m.clone());
            }
        }
    }
    out
}

fn parse_seq_bound(s: &str, messages: &[ImapMessage], by_uid: bool) -> u32 {
    if s == "*" {
        messages
            .iter()
            .map(|m| if by_uid { m.uid } else { m.seqno })
            .max()
            .unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}

fn memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_star() {
        assert!(match_wildcard("INBOX", "*"));
        assert!(match_wildcard("Sent", "*"));
        assert!(match_wildcard("Work.Client", "*"));
    }

    #[test]
    fn wildcard_matches_percent() {
        // Maildir has no hierarchy separator in our IMAP naming, so
        // % and * both collapse to "anything" per RFC 3501 §6.3.8.
        assert!(match_wildcard("INBOX", "%"));
        assert!(match_wildcard("Work.Client", "%"));
    }

    #[test]
    fn wildcard_matches_prefix() {
        assert!(match_wildcard("Sent", "S*"));
        assert!(!match_wildcard("Draft", "S*"));
    }

    #[test]
    fn parse_flags_reads_backslash_names() {
        let f = parse_imap_flags("(\\Seen \\Flagged)");
        assert!(f.contains(&Flag::Seen));
        assert!(f.contains(&Flag::Flagged));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn flags_to_imap_serialises_known() {
        let s = flags_to_imap(&[Flag::Seen, Flag::Flagged]);
        assert_eq!(s, "\\Seen \\Flagged");
    }
}
