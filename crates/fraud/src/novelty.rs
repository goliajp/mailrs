//! Whether a domain is new to this deployment: first seen within the last
//! week, after the deployment had already been watching for a week.
//!
//! The times come from `mailrs:domfirst:{registrable}` (the moment a
//! delivered message first carried the domain, as its sender or in a link)
//! and `mailrs:domfirst-since` (when this deployment began recording them).
//! Both are facts written once; this only reads them.
//!
//! A domain with no first-seen time is new only when nothing at all has
//! arrived from it (`domain_seen` zero) and the deployment has watched long
//! enough to know; a domain mail arrived from before recording began has a
//! count but no time, and is old. Without the store every answer is "not
//! new": missing evidence errs towards delivering.

/// A week, in seconds: how recently a domain must have appeared, and how
/// long recording must have run before anything counts as new.
pub const NEW_WITHIN: i64 = 7 * 86_400;

/// What the store says about the domains of one message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Novelty {
    /// Whole days from the sender's domain first appearing to the message;
    /// `None` when there is no first-seen time.
    pub sender_age_days: Option<i64>,
    /// Messages that arrived from the sender's domain before this one;
    /// zero when the store has no count.
    pub sender_seen: u64,
    /// The sender's registrable domain is new.
    pub sender_new: bool,
    /// The first of the message's link domains that is new.
    pub new_link_domain: Option<String>,
}

impl Novelty {
    /// The sender's domain has a history here: first seen more than
    /// [`ESTABLISHED_AFTER_DAYS`] days before the message, or
    /// [`ESTABLISHED_AT_MESSAGES`] messages from it already. False when
    /// the store could not be read.
    #[must_use]
    pub fn established(&self) -> bool {
        self.sender_age_days
            .is_some_and(|d| d > ESTABLISHED_AFTER_DAYS)
            || self.sender_seen >= ESTABLISHED_AT_MESSAGES
    }
}

/// See [`Novelty::established`].
pub const ESTABLISHED_AFTER_DAYS: i64 = 30;
/// See [`Novelty::established`].
pub const ESTABLISHED_AT_MESSAGES: u64 = 5;

/// Whether a domain first seen at `first` (`None`: never recorded) is new
/// for a message arriving at `at`, given recording began at `since`.
/// `seen` is how many messages arrived from it as a sender (zero for a
/// domain only ever linked to).
pub fn is_new(first: Option<i64>, seen: u64, since: Option<i64>, at: i64) -> bool {
    let Some(since) = since else {
        return false;
    };
    match first {
        Some(first) => at - first < NEW_WITHIN && first - since >= NEW_WITHIN,
        None => seen == 0 && at - since >= NEW_WITHIN,
    }
}

/// The message's novelty from the stored times. `links` are the link
/// domains with their first-seen times, in the order the message has them.
pub fn assess(
    sender: (Option<i64>, u64),
    links: &[(String, Option<i64>)],
    since: Option<i64>,
    at: i64,
) -> Novelty {
    let (sender_first, sender_seen) = sender;
    Novelty {
        sender_age_days: sender_first.map(|f| (at - f).max(0) / 86_400),
        sender_seen,
        sender_new: is_new(sender_first, sender_seen, since, at),
        new_link_domain: links
            .iter()
            .find(|(_, first)| is_new(*first, 0, since, at))
            .map(|(d, _)| d.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const SINCE: i64 = 1_000 * DAY;

    #[test]
    fn a_domain_first_seen_this_week_after_a_week_of_watching_is_new() {
        let at = SINCE + 30 * DAY;
        assert!(is_new(Some(at - DAY), 1, Some(SINCE), at));
        assert!(
            !is_new(Some(at - 8 * DAY), 3, Some(SINCE), at),
            "seen for over a week"
        );
    }

    #[test]
    fn nothing_is_new_in_the_first_week_of_recording() {
        let at = SINCE + 3 * DAY;
        assert!(!is_new(Some(at - DAY), 1, Some(SINCE), at));
        assert!(!is_new(None, 0, Some(SINCE), at));
    }

    #[test]
    fn a_domain_mail_came_from_before_recording_began_is_old() {
        let at = SINCE + 30 * DAY;
        assert!(!is_new(None, 12, Some(SINCE), at), "a count but no time");
        assert!(is_new(None, 0, Some(SINCE), at), "never seen at all");
    }

    #[test]
    fn without_the_store_nothing_is_new() {
        assert!(!is_new(None, 0, None, SINCE));
        assert_eq!(
            assess((None, 0), &[("x.test".into(), None)], None, SINCE),
            Novelty::default()
        );
    }

    #[test]
    fn the_first_new_link_domain_is_named() {
        let at = SINCE + 30 * DAY;
        let n = assess(
            (Some(SINCE), 40),
            &[
                ("old.test".into(), Some(SINCE)),
                ("fresh.test".into(), Some(at - DAY)),
            ],
            Some(SINCE),
            at,
        );
        assert_eq!(n.sender_age_days, Some(30));
        assert!(!n.sender_new);
        assert_eq!(n.new_link_domain.as_deref(), Some("fresh.test"));
    }
}
