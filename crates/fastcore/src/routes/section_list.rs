//! One section of the sectioned inbox, as a conversation list.
//!
//! `awaiting` comes off the `awaiting_since` index — it spans the Inbox
//! and Sent, ordered by when the reader's message went out, and its cursor
//! is that time — the others off the `by_user_section` ORDERPATH, newest
//! activity first, paged by activity as every list is. Held conversations
//! stay out, as on every ordinary list.

use mailrs_core_api::types::ConversationFilter;
use mailrs_mailbox_kevy::section::Section;
use mailrs_mailbox_kevy::{ArchiveScope, ThreadRow};

use crate::FastcoreState;

pub(crate) fn section_page(
    state: &FastcoreState,
    user: &str,
    section: Section,
    f: &ConversationFilter,
    limit: usize,
) -> std::io::Result<Vec<ThreadRow>> {
    let tids: Vec<String> = match section {
        Section::Awaiting => state
            .mailbox
            .list_awaiting_via_index(user, f.followup_due_by)?
            .into_iter()
            .filter(|(_, since)| f.before_ts.is_none_or(|ts| *since < ts))
            .map(|(tid, _)| tid)
            .collect(),
        _ => match f.before_ts {
            Some(ts) => state.mailbox.list_thread_ids_by_section_before_via_table(
                user,
                section,
                ArchiveScope::Live,
                ts - 1,
                limit,
            )?,
            None => state.mailbox.list_thread_ids_by_section_via_table(
                user,
                section,
                ArchiveScope::Live,
                limit,
            )?,
        },
    };
    let mut rows = Vec::with_capacity(limit);
    for tid in tids {
        if rows.len() == limit {
            break;
        }
        let Some(row) = state.mailbox.get_thread_for_user(user, &tid)? else {
            continue;
        };
        // the index has every waiting row; the list is the live ones
        if row.archived || row.quarantined || row.trashed_at > 0 {
            continue;
        }
        rows.push(row);
    }
    Ok(rows)
}
