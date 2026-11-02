//! The webhook event types a subscription can name, and which core-api
//! thread call is which thread event.

use crate::method::label::PATH_THREAD_LABEL;
use crate::method::thread::{
    PATH_ARCHIVE, PATH_MARK_READ, PATH_MARK_UNREAD, PATH_STAR, PATH_TRASH, PATH_UNARCHIVE,
    PATH_UNSTAR, PATH_UNTRASH,
};

pub const MAIL_RECEIVED: &str = "mail.received";
pub const MAIL_HELD: &str = "mail.held";
pub const MAIL_SENT: &str = "mail.sent";
pub const MAIL_DELIVERED: &str = "mail.delivered";
pub const MAIL_DEFERRED: &str = "mail.deferred";
pub const MAIL_BOUNCED: &str = "mail.bounced";
pub const THREAD_READ: &str = "thread.read";
pub const THREAD_ARCHIVED: &str = "thread.archived";
pub const THREAD_TRASHED: &str = "thread.trashed";
pub const THREAD_STARRED: &str = "thread.starred";
pub const THREAD_LABELLED: &str = "thread.labelled";
/// A team conversation was assigned to a member, or the assignment taken
/// back. Carries the group, the conversation and the assignee; never a
/// note.
pub const THREAD_ASSIGNED: &str = "thread.assigned";
pub const CALENDAR_INVITE_ANSWERED: &str = "calendar.invite_answered";

/// The old name of [`MAIL_RECEIVED`]; subscriptions stored under it keep
/// working and keep receiving payloads that say `new_message`.
pub const NEW_MESSAGE: &str = "new_message";

/// Every event type a user may subscribe to.
pub const ALL: [&str; 13] = [
    MAIL_RECEIVED,
    MAIL_HELD,
    MAIL_SENT,
    MAIL_DELIVERED,
    MAIL_DEFERRED,
    MAIL_BOUNCED,
    THREAD_READ,
    THREAD_ARCHIVED,
    THREAD_TRASHED,
    THREAD_STARRED,
    THREAD_LABELLED,
    THREAD_ASSIGNED,
    CALENDAR_INVITE_ANSWERED,
];

/// Whether a subscription may be created for `event_type`.
pub fn is_known(event_type: &str) -> bool {
    event_type == NEW_MESSAGE || ALL.contains(&event_type)
}

/// Whether a subscription stored as `subscribed` receives `event`.
pub fn subscribed_to(subscribed: &str, event: &str) -> bool {
    subscribed == event || (subscribed == NEW_MESSAGE && event == MAIL_RECEIVED)
}

/// A thread's state changed through a core-api call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadChange {
    pub event: &'static str,
    /// The data field that carries the new state.
    pub field: &'static str,
    pub on: bool,
}

/// The thread event a successful call to `template` (the route's path
/// template) with `method` stands for; `None` for every other call.
pub fn thread_event(template: &str, method: &str) -> Option<ThreadChange> {
    let change = |event, field, on| Some(ThreadChange { event, field, on });
    match (template, method) {
        (PATH_MARK_READ, "POST") => change(THREAD_READ, "read", true),
        (PATH_MARK_UNREAD, "POST") => change(THREAD_READ, "read", false),
        (PATH_ARCHIVE, "POST") => change(THREAD_ARCHIVED, "archived", true),
        (PATH_UNARCHIVE, "POST") => change(THREAD_ARCHIVED, "archived", false),
        (PATH_TRASH, "POST") => change(THREAD_TRASHED, "trashed", true),
        (PATH_UNTRASH, "POST") => change(THREAD_TRASHED, "trashed", false),
        (PATH_STAR, "POST") => change(THREAD_STARRED, "starred", true),
        (PATH_UNSTAR, "POST") => change(THREAD_STARRED, "starred", false),
        (PATH_THREAD_LABEL, "POST") => change(THREAD_LABELLED, "labelled", true),
        (PATH_THREAD_LABEL, "DELETE") => change(THREAD_LABELLED, "labelled", false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_thread_call_is_its_event_and_nothing_else_is() {
        let read = thread_event(PATH_MARK_READ, "POST").unwrap();
        assert_eq!(
            (read.event, read.field, read.on),
            (THREAD_READ, "read", true)
        );
        assert!(!thread_event(PATH_MARK_UNREAD, "POST").unwrap().on);
        assert_eq!(
            thread_event(PATH_UNTRASH, "POST").unwrap().event,
            THREAD_TRASHED
        );
        assert!(!thread_event(PATH_THREAD_LABEL, "DELETE").unwrap().on);
        assert_eq!(thread_event(PATH_ARCHIVE, "GET"), None);
        assert_eq!(
            thread_event("/v1/users/{user}/threads/{thread_id}/messages", "POST"),
            None
        );
    }

    #[test]
    fn the_old_name_is_kept_and_unknown_names_are_not() {
        assert!(is_known(NEW_MESSAGE));
        assert!(is_known(MAIL_BOUNCED));
        assert!(!is_known("mail.new"));
        assert!(!is_known("mock_message"));
        assert!(subscribed_to(NEW_MESSAGE, MAIL_RECEIVED));
        assert!(!subscribed_to(NEW_MESSAGE, MAIL_HELD));
        assert!(!subscribed_to(MAIL_RECEIVED, MAIL_SENT));
    }
}
