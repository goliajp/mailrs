//! IMAP SEARCH and SORT handlers plus the message-matching
//! engine they share.
//!
//! `SortCriterion` is the parsed form of the SORT command's
//! ordering list (with REVERSE modifiers); `parse_sort_criteria`
//! parses it from the wire string; `message_matches_criteria`
//! evaluates an AND-list of [`SearchKey`]s against one message.
//! Both are `pub(super)` because tests in `tests.rs` exercise
//! them directly.

use mailrs_imap_proto::{
    format_no, format_ok, parse_search_criteria, parse_sequence_set, sequence_set_to_uids, SearchKey,
};
use mailrs_mailbox::{
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
};

use super::{ImapSession, ImapState};

/// One ordering criterion for the SORT command, including the
/// REVERSE-prefixed variants from RFC 5256. Used by
/// [`ImapSession::handle_sort`] to compare two messages.
pub(super) enum SortCriterion {
    Arrival,
    Date,
    From,
    Subject,
    Size,
    ReverseArrival,
    ReverseDate,
    ReverseFrom,
    ReverseSubject,
    ReverseSize,
}

/// Parse a SORT criteria string like `"REVERSE DATE"` or
/// `"ARRIVAL"` into a list of [`SortCriterion`]s (primary first).
/// Unknown tokens are silently skipped — the IMAP wire never
/// allows for protocol-level error reporting from SORT.
pub(super) fn parse_sort_criteria(criteria: &str) -> Vec<SortCriterion> {
    let tokens: Vec<&str> = criteria.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i].to_uppercase();
        if token == "REVERSE" && i + 1 < tokens.len() {
            let next = tokens[i + 1].to_uppercase();
            match next.as_str() {
                "ARRIVAL" => result.push(SortCriterion::ReverseArrival),
                "DATE" => result.push(SortCriterion::ReverseDate),
                "FROM" => result.push(SortCriterion::ReverseFrom),
                "SUBJECT" => result.push(SortCriterion::ReverseSubject),
                "SIZE" | "RFC822.SIZE" => result.push(SortCriterion::ReverseSize),
                _ => {}
            }
            i += 2;
        } else {
            match token.as_str() {
                "ARRIVAL" => result.push(SortCriterion::Arrival),
                "DATE" => result.push(SortCriterion::Date),
                "FROM" => result.push(SortCriterion::From),
                "SUBJECT" => result.push(SortCriterion::Subject),
                "SIZE" | "RFC822.SIZE" => result.push(SortCriterion::Size),
                _ => {}
            }
            i += 1;
        }
    }
    result
}

/// Evaluate an AND-list of [`SearchKey`] criteria against one
/// message's metadata. Returns true iff every key matches.
///
/// Date keys (`SINCE`/`BEFORE`/`ON`) work in seconds-since-epoch
/// (`msg.date`); `ON` widens the timestamp to a 86_400-second
/// day window. Text keys (`FROM`/`TO`/`SUBJECT`/`TEXT`) compare
/// case-insensitively. `BODY` is approximated by `SUBJECT`
/// because reading the message body would cost a Maildir disk
/// read per message — opt-in expensive matching is a future
/// enhancement, not a regression of this refactor.
pub(super) fn message_matches_criteria(
    msg: &mailrs_mailbox::MessageMeta,
    keys: &[SearchKey],
) -> bool {
    // seconds per day for date comparisons
    const DAY: i64 = 86400;

    for key in keys {
        let matches = match key {
            SearchKey::All => true,
            SearchKey::Seen => msg.flags & FLAG_SEEN != 0,
            SearchKey::Unseen => msg.flags & FLAG_SEEN == 0,
            SearchKey::Flagged => msg.flags & FLAG_FLAGGED != 0,
            SearchKey::Unflagged => msg.flags & FLAG_FLAGGED == 0,
            SearchKey::Answered => msg.flags & FLAG_ANSWERED != 0,
            SearchKey::Unanswered => msg.flags & FLAG_ANSWERED == 0,
            SearchKey::Deleted => msg.flags & FLAG_DELETED != 0,
            SearchKey::Undeleted => msg.flags & FLAG_DELETED == 0,
            SearchKey::Draft => msg.flags & FLAG_DRAFT != 0,
            SearchKey::Undraft => msg.flags & FLAG_DRAFT == 0,
            SearchKey::Recent => msg.flags & FLAG_RECENT != 0,
            SearchKey::From(pattern) => {
                msg.sender.to_lowercase().contains(&pattern.to_lowercase())
            }
            SearchKey::To(pattern) => msg
                .recipients
                .to_lowercase()
                .contains(&pattern.to_lowercase()),
            SearchKey::Subject(pattern) => msg
                .subject
                .to_lowercase()
                .contains(&pattern.to_lowercase()),
            SearchKey::Text(pattern) => {
                let p = pattern.to_lowercase();
                msg.subject.to_lowercase().contains(&p)
                    || msg.sender.to_lowercase().contains(&p)
                    || msg.recipients.to_lowercase().contains(&p)
            }
            SearchKey::Body(pattern) => {
                // body search requires reading message content, which is expensive
                // fall back to subject search as a best-effort approximation
                msg.subject
                    .to_lowercase()
                    .contains(&pattern.to_lowercase())
            }
            SearchKey::Since(ts) => msg.date >= *ts,
            SearchKey::Before(ts) => msg.date < *ts,
            SearchKey::On(ts) => {
                let day_start = *ts;
                let day_end = day_start + DAY;
                msg.date >= day_start && msg.date < day_end
            }
            SearchKey::Uid(seq_str) => match parse_sequence_set(seq_str) {
                Ok(set) => {
                    let uids = sequence_set_to_uids(&set, u32::MAX);
                    uids.contains(&msg.uid)
                }
                Err(_) => false,
            },
        };
        if !matches {
            return false;
        }
    }
    true
}

impl ImapSession {
    pub(super) async fn handle_search(&self, tag: &str, criteria: &str) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "SEARCH failed")],
        };

        let keys = parse_search_criteria(criteria);
        let mut matching_seqs: Vec<u32> = Vec::new();

        for (i, msg) in messages.iter().enumerate() {
            let seq = i as u32 + 1;
            if message_matches_criteria(msg, &keys) {
                matching_seqs.push(seq);
            }
        }

        let seq_list: Vec<String> = matching_seqs.iter().map(|s| s.to_string()).collect();
        vec![
            format!("* SEARCH {}\r\n", seq_list.join(" ")),
            format_ok(tag, "SEARCH completed"),
        ]
    }

    pub(super) async fn handle_sort(
        &self,
        tag: &str,
        criteria: &str,
        search_criteria: &str,
        uid_mode: bool,
    ) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "SORT failed")],
        };

        // filter by search criteria
        let keys = parse_search_criteria(search_criteria);
        let mut filtered: Vec<(usize, &mailrs_mailbox::MessageMeta)> = messages
            .iter()
            .enumerate()
            .filter(|(_, msg)| message_matches_criteria(msg, &keys))
            .collect();

        // sort by criteria
        let sort_keys = parse_sort_criteria(criteria);
        filtered.sort_by(|a, b| {
            for key in &sort_keys {
                let ord = match key {
                    SortCriterion::Arrival => a.1.internal_date.cmp(&b.1.internal_date),
                    SortCriterion::Date => a.1.date.cmp(&b.1.date),
                    SortCriterion::From => {
                        a.1.sender.to_lowercase().cmp(&b.1.sender.to_lowercase())
                    }
                    SortCriterion::Subject => {
                        a.1.subject.to_lowercase().cmp(&b.1.subject.to_lowercase())
                    }
                    SortCriterion::Size => a.1.size.cmp(&b.1.size),
                    SortCriterion::ReverseArrival => {
                        b.1.internal_date.cmp(&a.1.internal_date)
                    }
                    SortCriterion::ReverseDate => b.1.date.cmp(&a.1.date),
                    SortCriterion::ReverseFrom => {
                        b.1.sender.to_lowercase().cmp(&a.1.sender.to_lowercase())
                    }
                    SortCriterion::ReverseSubject => {
                        b.1.subject.to_lowercase().cmp(&a.1.subject.to_lowercase())
                    }
                    SortCriterion::ReverseSize => b.1.size.cmp(&a.1.size),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });

        let ids: Vec<String> = if uid_mode {
            filtered
                .iter()
                .map(|(_, msg)| msg.uid.to_string())
                .collect()
        } else {
            filtered.iter().map(|(i, _)| (i + 1).to_string()).collect()
        };

        vec![
            format!("* SORT {}\r\n", ids.join(" ")),
            format_ok(tag, "SORT completed"),
        ]
    }
}
