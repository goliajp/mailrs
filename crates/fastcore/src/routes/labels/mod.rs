//! The list of one label's conversations, and the label ids every listed
//! row carries.

use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox_kevy::{ListThreadsFilter, ThreadRow};

use crate::FastcoreState;

/// One label's conversations, newest first: read from the label's set and
/// sorted here. Archived ones are in it, as a label is not a place; those
/// in the Trash are not, and held ones only when asked for.
pub(crate) fn rows_with_label(
    state: &FastcoreState,
    user: &str,
    id: &str,
    filter: &ListThreadsFilter<'_>,
    limit: usize,
) -> std::io::Result<Vec<ThreadRow>> {
    let mut rows = Vec::new();
    for tid in state.mailbox.threads_with_label(user, id)? {
        let Some(row) = state.mailbox.get_thread_for_user(user, &tid)? else {
            continue;
        };
        let held =
            row.quarantined && filter.quarantine != mailrs_mailbox_kevy::QuarantineScope::Include;
        let shown = row.trashed_at == 0
            && !held
            && filter.before_ts.is_none_or(|ts| row.latest_date < ts)
            && (!filter.has_unread || row.unread_count > 0)
            && (!filter.starred || row.starred);
        if shown {
            rows.push(row);
        }
    }
    // newest first, then by id: the order the SQL core gives the same rows
    rows.sort_by(|a, b| {
        b.latest_date
            .cmp(&a.latest_date)
            .then(a.thread_id.cmp(&b.thread_id))
    });
    rows.truncate(limit);
    Ok(rows)
}

/// `items` with each one's label ids, from one read per label.
pub(crate) fn with_labels(
    state: &FastcoreState,
    user: &str,
    mut items: Vec<ConversationSummaryWire>,
) -> Vec<ConversationSummaryWire> {
    let by_thread = match state.mailbox.labels_by_thread(user) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(%user, error = %e, "labels not read; rows shown without them");
            return items;
        }
    };
    for item in &mut items {
        if let Some(ids) = by_thread.get(&item.thread_id) {
            item.labels = ids.clone();
        }
    }
    items
}
