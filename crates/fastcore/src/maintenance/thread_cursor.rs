//! Paging a walk over every user's threads by the last thread done rather
//! than by a count of what came before it.
//!
//! An offset counts positions in a list that changes under it: a thread
//! created or deleted ahead of the offset while a backfill runs shifts
//! everything after it, so the next page repeats some threads and skips
//! others. A cursor names the last (user, thread) a page finished; the
//! next page starts after it in sorted order, wherever the list has grown
//! or shrunk in between. A thread that arrives behind the cursor is not
//! visited by this walk — delivery itself records it.

/// Where the previous page stopped: the last thread it finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct After {
    pub user: String,
    pub thread: String,
}

impl After {
    /// From the two query parameters; both or nothing.
    pub(crate) fn from_query(user: Option<&str>, thread: Option<&str>) -> Option<Self> {
        Some(Self {
            user: user?.to_string(),
            thread: thread?.to_string(),
        })
    }
}

/// The users a page walks, in order, from the cursor's user on.
pub(crate) fn users_from(mut users: Vec<String>, after: Option<&After>) -> Vec<String> {
    users.sort();
    match after {
        Some(a) => users.into_iter().filter(|u| *u >= a.user).collect(),
        None => users,
    }
}

/// `user`'s threads after the cursor, in order.
pub(crate) fn threads_after(
    state: &crate::FastcoreState,
    user: &str,
    after: Option<&After>,
) -> Vec<String> {
    let all = state
        .mailbox
        .all_thread_ids_for_user(user)
        .unwrap_or_default();
    match after {
        Some(a) if a.user == user => all.into_iter().filter(|t| *t > a.thread).collect(),
        _ => all,
    }
}

/// The cursor a page hands back: where the next one starts.
pub(crate) fn next(last: &Option<After>) -> serde_json::Value {
    match last {
        Some(a) => serde_json::json!({ "user": a.user, "thread": a.thread }),
        None => serde_json::Value::Null,
    }
}
