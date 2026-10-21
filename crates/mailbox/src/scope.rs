//! Which conversations a view shows: the triage buckets, and how a
//! request's `folder` and `category` combine into one scope.
//!
//! Both stores answer the same views, and when each spelled this out for
//! itself they drifted: one listed sent-only threads in Inbox, one sent
//! `nonjunk` to an empty mailbox-name match, and a store's own list and
//! search disagreed on whether `nonjunk` held sent-only threads. So the
//! rules live here once. A store translates a [`Scope`](crate::scope::Scope) into its own query,
//! and [`Scope::admits`](crate::scope::Scope::admits) is the row-level statement of the same rule that
//! a translation is tested against.

/// The triage bucket a thread belongs to. Exactly one holds every thread;
/// Sent, archived, starred and pinned are separate axes. The bucket is a
/// function of the thread's category alone, see [`bucket_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// Person-to-person mail, and anything not classified otherwise.
    Inbox,
    /// Machine-sent notices: receipts, alerts, confirmations.
    Notifications,
    /// Marketing.
    Promotions,
    /// Spam and scams.
    Junk,
}

impl Bucket {
    /// The category a thread is given when it is filed into this bucket.
    pub fn category(self) -> &'static str {
        match self {
            Bucket::Inbox => "inbox",
            Bucket::Notifications => "notification",
            Bucket::Promotions => "promotion",
            Bucket::Junk => "spam",
        }
    }

    /// The bucket's own name. Several categories map onto one bucket, so
    /// this is not a category.
    pub fn name(self) -> &'static str {
        match self {
            Bucket::Inbox => "inbox",
            Bucket::Notifications => "notifications",
            Bucket::Promotions => "promotions",
            Bucket::Junk => "junk",
        }
    }
}

/// The buckets real mail is filed into: every one but Junk.
pub const NON_JUNK: [Bucket; 3] = [Bucket::Inbox, Bucket::Notifications, Bucket::Promotions];

/// The categories that put a thread outside Inbox, compared without case.
/// A store that matches categories in its own query language builds the
/// match from this table, so it cannot disagree with [`bucket_of`].
pub const CATEGORY_BUCKETS: [(&str, Bucket); 6] = [
    ("spam", Bucket::Junk),
    ("scam", Bucket::Junk),
    ("notification", Bucket::Notifications),
    ("notifications", Bucket::Notifications),
    ("promotion", Bucket::Promotions),
    ("promotions", Bucket::Promotions),
];

/// The bucket a category puts a thread in. Unknown and empty categories
/// are Inbox.
pub fn bucket_of(category: &str) -> Bucket {
    let c = category.trim();
    CATEGORY_BUCKETS
        .iter()
        .find(|(name, _)| c.eq_ignore_ascii_case(name))
        .map_or(Bucket::Inbox, |(_, b)| *b)
}

/// What a view's `folder` and `category` scope it to, once reconciled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// No folder and no category: every thread.
    All,
    /// Every bucket but Junk. The scope of the cross-cutting views, Unread
    /// and Starred, which are attributes of a thread rather than places;
    /// junk mail has exactly one surface.
    NotJunk,
    /// One bucket.
    Bucket(Bucket),
    /// Notifications and promotions together.
    Np,
    /// One category, which implies its bucket and is narrower than it.
    Category(String),
    /// A folder and a category that cannot both hold: no thread does.
    Contradiction,
}

/// Reconcile a view's `folder` and `category`.
///
/// `sent` is not a scope, it is the is-sender axis (see [`is_sent_folder`]);
/// a folder name this does not know scopes nothing.
pub fn scope(folder: Option<&str>, category: Option<&str>) -> Scope {
    let named = |f: &str| folder.is_some_and(|x| x.eq_ignore_ascii_case(f));
    let folder_bucket = if named("inbox") {
        Some(Bucket::Inbox)
    } else if named("junk") {
        Some(Bucket::Junk)
    } else if named("notifications") {
        Some(Bucket::Notifications)
    } else if named("promotions") {
        Some(Bucket::Promotions)
    } else {
        None
    };
    if named("nonjunk") {
        return match category {
            Some(c) if bucket_of(c) == Bucket::Junk => Scope::Contradiction,
            Some(c) => Scope::Category(c.to_string()),
            None => Scope::NotJunk,
        };
    }
    let np = named("np");
    match (category, folder_bucket) {
        (Some(c), Some(b)) if bucket_of(c) != b => Scope::Contradiction,
        (Some(c), _)
            if np && !matches!(bucket_of(c), Bucket::Notifications | Bucket::Promotions) =>
        {
            Scope::Contradiction
        }
        (Some(c), _) => Scope::Category(c.to_string()),
        (None, Some(b)) => Scope::Bucket(b),
        (None, None) if np => Scope::Np,
        (None, None) => Scope::All,
    }
}

/// Whether `folder` names the Sent view, the threads the user wrote into.
pub fn is_sent_folder(folder: Option<&str>) -> bool {
    folder.is_some_and(|f| f.eq_ignore_ascii_case("sent"))
}

/// The facts about one (user, thread) a scope decides on.
#[derive(Debug, Clone, Copy)]
pub struct ThreadFacts<'a> {
    /// The thread's category for this user.
    pub category: &'a str,
    /// Every message in it is one the user sent.
    pub sent_only: bool,
}

impl Scope {
    /// Whether a thread with these facts is in the scope. A thread the user
    /// only ever sent into is in no Inbox, on any axis.
    pub fn admits(&self, t: ThreadFacts<'_>) -> bool {
        let bucket = bucket_of(t.category);
        let in_bucket = |b: Bucket| bucket == b && !(b == Bucket::Inbox && t.sent_only);
        match self {
            Scope::All => true,
            Scope::NotJunk => NON_JUNK.into_iter().any(in_bucket),
            Scope::Bucket(b) => in_bucket(*b),
            Scope::Np => in_bucket(Bucket::Notifications) || in_bucket(Bucket::Promotions),
            Scope::Category(c) => t.category.eq_ignore_ascii_case(c),
            Scope::Contradiction => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_category_implies_its_bucket_or_contradicts_the_folder() {
        assert_eq!(scope(Some("Junk"), Some("promotion")), Scope::Contradiction);
        assert_eq!(scope(Some("NP"), Some("inbox")), Scope::Contradiction);
        assert_eq!(scope(Some("nonjunk"), Some("spam")), Scope::Contradiction);
        assert_eq!(
            scope(Some("Inbox"), Some("inbox")),
            Scope::Category("inbox".into())
        );
        assert_eq!(scope(Some("NP"), None), Scope::Np);
        assert_eq!(scope(Some("nonjunk"), None), Scope::NotJunk);
        assert_eq!(scope(Some("Drafts"), None), Scope::All);
        assert_eq!(scope(None, None), Scope::All);
    }

    #[test]
    fn a_sent_only_thread_is_in_no_inbox() {
        let sent = ThreadFacts {
            category: "inbox",
            sent_only: true,
        };
        assert!(!Scope::Bucket(Bucket::Inbox).admits(sent));
        assert!(!Scope::NotJunk.admits(sent));
        assert!(Scope::All.admits(sent));
        let note = ThreadFacts {
            category: "notification",
            sent_only: true,
        };
        assert!(Scope::Np.admits(note));
        assert!(Scope::NotJunk.admits(note));
    }

    #[test]
    fn unknown_categories_are_inbox() {
        assert_eq!(bucket_of(""), Bucket::Inbox);
        assert_eq!(bucket_of("general"), Bucket::Inbox);
        assert_eq!(bucket_of("Notifications"), Bucket::Notifications);
        assert_eq!(bucket_of("scam"), Bucket::Junk);
    }
}
