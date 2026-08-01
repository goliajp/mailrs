//! FETCH and STORE — reading a message and changing its flags.
//!
//! Maildir encodes flags into the filename, so `parse_imap_flags` and
//! `flags_to_imap` are the two halves of one translation and belong
//! beside each other.

use mailrs_imap_proto::{format_fetch, format_no, format_ok};
use std::sync::Arc;

use mailrs_maildir::Flag;

use super::backend::{self, ImapMessage};
use crate::FastcoreState;

use super::query::*;
use super::session::State;

pub(super) fn fetch_response(
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

pub(super) fn fetch_items(msg: &ImapMessage, attrs: &str, by_uid: bool) -> Vec<(String, String)> {
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

pub(super) fn flags_to_imap(flags: &[Flag]) -> String {
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

pub(super) fn parse_imap_flags(s: &str) -> Vec<Flag> {
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

pub(super) fn format_internal_date(epoch: i64) -> String {
    use chrono::{DateTime, Utc};
    match DateTime::<Utc>::from_timestamp(epoch, 0) {
        Some(dt) => dt.format("%d-%b-%Y %H:%M:%S +0000").to_string(),
        None => "01-Jan-1970 00:00:00 +0000".to_string(),
    }
}

pub(super) fn store_response(
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

pub(super) fn expunge(
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
