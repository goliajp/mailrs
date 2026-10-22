//! SEARCH, SORT, QUOTA, and the sequence-set arithmetic they share.

use mailrs_imap_proto::{SearchKey, format_no, format_ok};

use super::search_eval::{self, MsgView};
use super::session::{State, no};
use crate::backend::{Account, Error, Message};

/// Each message with its bytes, read only when `keys` look inside them.
async fn with_bytes<'a, A: Account>(
    account: &A,
    messages: &'a [Message<A::Key>],
    content: bool,
) -> Result<Vec<(&'a Message<A::Key>, Vec<u8>)>, Error> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let bytes = match content {
            true => account.read(m).await?.unwrap_or_default(),
            false => Vec::new(),
        };
        out.push((m, bytes));
    }
    Ok(out)
}

fn matching<'a, K>(
    keys: &[SearchKey],
    loaded: &'a [(&'a Message<K>, Vec<u8>)],
) -> Vec<&'a (&'a Message<K>, Vec<u8>)> {
    loaded
        .iter()
        .filter(|(m, bytes)| {
            let v = MsgView::new(m, bytes);
            keys.iter().all(|k| search_eval::matches(k, &v))
        })
        .collect()
}

fn listing(word: &str, numbers: impl Iterator<Item = u32>) -> String {
    let list = numbers.map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
    match list.is_empty() {
        true => format!("* {word}\r\n"),
        false => format!("* {word} {list}\r\n"),
    }
}

pub(super) async fn search_response<A: Account>(
    session: &State<A>,
    tag: &str,
    criteria: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected {
        account, messages, ..
    } = session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    // full RFC 3501 grammar: implicit AND, OR / NOT, parenthesized
    // groups, HEADER / LARGER / SMALLER, dates, UID sets
    let keys = mailrs_imap_proto::parse_search_criteria(criteria);
    let content = search_eval::reads_content(&keys);
    let loaded = match with_bytes(&**account, messages, content).await {
        Ok(l) => l,
        Err(e) => return vec![no(tag, "SEARCH", &e)],
    };
    let found = matching(&keys, &loaded)
        .into_iter()
        .map(|(m, _)| if by_uid { m.uid } else { m.seqno });
    vec![listing("SEARCH", found), format_ok(tag, "SEARCH completed")]
}

/// `SORT (criteria) charset search-keys` (RFC 5256) — filter with the
/// same evaluator SEARCH uses, then order per criteria.
pub(super) async fn sort_response<A: Account>(
    session: &State<A>,
    tag: &str,
    criteria: &str,
    search: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected {
        account, messages, ..
    } = session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    let keys = mailrs_imap_proto::parse_search_criteria(search);
    let (reverse, crits) = search_eval::parse_sort_criteria(criteria);
    let content = search_eval::reads_content(&keys) || search_eval::sorts_on_headers(&crits);
    let loaded = match with_bytes(&**account, messages, content).await {
        Ok(l) => l,
        Err(e) => return vec![no(tag, "SORT", &e)],
    };
    let views: Vec<MsgView<'_, A::Key>> = matching(&keys, &loaded)
        .into_iter()
        .map(|(m, bytes)| MsgView::new(m, bytes))
        .collect();
    let sorted = search_eval::sort(views, reverse, &crits)
        .into_iter()
        .map(|v| if by_uid { v.msg().uid } else { v.msg().seqno });
    vec![listing("SORT", sorted), format_ok(tag, "SORT completed")]
}

/// `GETQUOTA` / the quota half of `GETQUOTAROOT` (RFC 2087). STORAGE
/// units are KiB per the RFC. No limit → empty resource list.
pub(super) async fn quota_response<A: Account>(session: &State<A>, tag: &str) -> Vec<String> {
    let Some(account) = session.account() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let quota = match account.quota().await {
        Ok(q) => q,
        Err(e) => return vec![no(tag, "GETQUOTA", &e)],
    };
    let line = match quota.limit > 0 {
        true => format!(
            "* QUOTA \"\" (STORAGE {} {})\r\n",
            quota.used / 1024,
            quota.limit / 1024
        ),
        false => "* QUOTA \"\" ()\r\n".to_string(),
    };
    vec![line, format_ok(tag, "GETQUOTA completed")]
}

/// Expand an IMAP sequence set (`1:5`, `*`, `2,5,8:10`) to matching
/// messages. Uses seqno when `by_uid` is false, UID otherwise.
pub(super) fn expand_sequence<K: Clone>(
    spec: &str,
    messages: &[Message<K>],
    by_uid: bool,
) -> Vec<Message<K>> {
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

fn parse_seq_bound<K>(s: &str, messages: &[Message<K>], by_uid: bool) -> u32 {
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
