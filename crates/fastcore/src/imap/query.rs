//! SEARCH, SORT, COPY, QUOTA, and the sequence-set arithmetic they share.

use mailrs_imap_proto::{format_no, format_ok};
use std::sync::Arc;

use super::backend::{self, ImapMessage};
use crate::FastcoreState;

use super::session::State;

pub(super) fn search_response(
    session: &State,
    tag: &str,
    criteria: &str,
    by_uid: bool,
) -> Vec<String> {
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
pub(super) fn sort_response(
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
pub(super) fn quota_response(
    _state: &Arc<FastcoreState>,
    session: &State,
    tag: &str,
) -> Vec<String> {
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

pub(super) fn copy_response(
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
        if backend::copy_to(state, &user, msg, &dest).is_err() {
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

/// Expand an IMAP sequence set (`1:5`, `*`, `2,5,8:10`) to matching
/// messages. Uses seqno when `by_uid` is false, UID otherwise.
pub(super) fn expand_sequence(
    spec: &str,
    messages: &[ImapMessage],
    by_uid: bool,
) -> Vec<ImapMessage> {
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

pub(super) fn parse_seq_bound(s: &str, messages: &[ImapMessage], by_uid: bool) -> u32 {
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

pub(super) fn memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
