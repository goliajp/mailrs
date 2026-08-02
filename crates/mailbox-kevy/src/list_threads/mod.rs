//! `list_threads_by_activity` — every conversation-list page.
//!
//! One dispatcher over the declared `threaduser` table. A filter names a
//! scope (a folder, a category, or neither) and any number of boolean
//! flags; this picks the axis to key on and hands the rest to the engine
//! as value filters.
//!
//! It is deliberately **total**. The previous shape enumerated the
//! combinations it knew — one bare flag, one flag plus one scope — and let
//! everything else fall through to the hand-maintained zsets. Once those
//! were dropped that fall-through returned an empty page: two flags at once
//! ("starred" plus "unread only"), or a folder and a category together,
//! answered `[]` with a 200 and no way to tell it from an empty mailbox.
//! Measured 2026-08-01: all fifteen legacy zsets held 0 rows across all 13
//! prod accounts, so every one of those combinations was already blank.
//!
//! The one shape that genuinely has no rows — a folder and a category that
//! disagree, say Junk ∩ `promotion` — returns empty because it *is* empty,
//! and says so in one place rather than by falling off the end.

use std::io;

use super::KevyMailboxStore;
use super::keys;
use super::thread_row::ThreadRow;

/// Filter knobs passed to `list_threads_by_activity`. None of these are
/// required; default is "all threads sorted by recency, latest first."
#[derive(Debug, Clone, Default)]
pub struct ListThreadsFilter<'a> {
    /// Restrict to a single category (`inbox`, `social`, `spam`, …) —
    /// the `category` column on the membership row.
    pub category: Option<&'a str>,
    /// Match monolith's `folder` query: `inbox`, `junk`, `notifications`,
    /// `promotions`, `np` (the merged notifications+promotions view) or
    /// `sent`, case-insensitively. Anything else is no scope at all.
    pub folder: Option<&'a str>,
    /// Only threads with `pinned = true`.
    pub pinned: bool,
    /// Only threads with `archived = true`.
    pub archived: bool,
    /// Only threads with `unread_count > 0`.
    pub has_unread: bool,
    /// Only threads with `has_action = true`.
    pub has_action: bool,
    /// Only threads with `starred = true`.
    pub starred: bool,
    /// Cursor for pagination: only return threads with `latest_date <
    /// before_ts`. `activity` is the component right after the equality
    /// columns in every declared composite, which is the one position a
    /// range may constrain — so this is the shape the ORDERPATHs were
    /// designed for. When `None`, the caller windows with
    /// `(offset, limit)`.
    pub before_ts: Option<i64>,
}

/// What the filter's `folder` / `category` pair scopes the page to,
/// after the two have been reconciled.
#[derive(Debug, Clone, PartialEq)]
enum Scope {
    /// No folder and no category — every thread the user has.
    All,
    /// One folder bucket: `inbox`, `junk`, `notifications`, `promotions`.
    Bucket(&'static str),
    /// The merged notifications+promotions view, which is two ranges.
    Np,
    /// One category, which implies its bucket and is narrower than it.
    Category(String),
    /// A folder and a category that cannot both hold — `junk` with
    /// `promotion`, say. No thread satisfies it, and that is an answer.
    Contradiction,
}

impl ListThreadsFilter<'_> {
    /// Every boolean flag the filter has switched on, as the column
    /// names the membership row stores them under, **first one first**.
    ///
    /// `sent` is a folder to the caller and a flag here: `is_sender` is
    /// a declared column like the other five, so treating it as one
    /// means "Sent, unread only" needs no special case.
    ///
    /// It is first on purpose. Each flag index stores the other columns
    /// beside it so a second predicate can be a FILTER rather than an
    /// intersection — and `is_sender` is the one column that list omits
    /// (`thread_user_spec`, asserted by `is_sender_is_the_only_flag_that_must_be_the_key`).
    /// So it can be keyed on but not filtered on, and the caller keys on
    /// whichever flag comes back first.
    fn flags_on(&self) -> Vec<&'static str> {
        let mut out = Vec::with_capacity(6);
        for (name, on) in [
            (
                "is_sender",
                self.folder.is_some_and(|f| f.eq_ignore_ascii_case("sent")),
            ),
            ("starred", self.starred),
            ("archived", self.archived),
            ("pinned", self.pinned),
            ("unread", self.has_unread),
            ("has_action", self.has_action),
        ] {
            if on {
                out.push(name);
            }
        }
        out
    }

    /// Reconcile `folder` and `category` into the one scope the page is
    /// keyed on.
    ///
    /// A category implies its bucket — `bucket_of` maps every category
    /// onto exactly one — so naming both is either redundant (and the
    /// category is the narrower of the two) or impossible. Saying which
    /// here is what keeps the dispatcher total.
    fn scope(&self) -> Scope {
        let folder_bucket = self.folder.and_then(|f| match f {
            f if f.eq_ignore_ascii_case("inbox") => Some(keys::Bucket::Inbox),
            f if f.eq_ignore_ascii_case("junk") => Some(keys::Bucket::Junk),
            f if f.eq_ignore_ascii_case("notifications") => Some(keys::Bucket::Notifications),
            f if f.eq_ignore_ascii_case("promotions") => Some(keys::Bucket::Promotions),
            _ => None,
        });
        let is_np = self.folder.is_some_and(|f| f.eq_ignore_ascii_case("np"));

        match (self.category, folder_bucket, is_np) {
            (Some(cat), Some(b), _) if keys::bucket_of(cat) != b => Scope::Contradiction,
            (Some(cat), _, true)
                if !matches!(
                    keys::bucket_of(cat),
                    keys::Bucket::Notifications | keys::Bucket::Promotions
                ) =>
            {
                Scope::Contradiction
            }
            (Some(cat), _, _) => Scope::Category(cat.to_string()),
            (None, Some(b), _) => Scope::Bucket(b.name()),
            (None, None, true) => Scope::Np,
            (None, None, false) => Scope::All,
        }
    }
}

impl KevyMailboxStore {
    /// List threads for `user` in reverse-activity order, with optional
    /// filter. `offset` skips the first N matches; `limit` caps the
    /// returned row count.
    ///
    /// Returns `(rows, total_in_index)`. `total_in_index` is the
    /// pre-pagination count of the chosen index — exactly the
    /// "X / Y conversations" badge the UI shows.
    pub fn list_threads_by_activity(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        // One dispatcher, total over the filter space. Every branch ends
        // in a declared index; nothing falls through, because what it
        // used to fall through to — the hand-maintained zsets — is gone,
        // and answered every combination it covered with an empty page.
        let scope = filter.scope();
        if scope == Scope::Contradiction {
            return Ok((Vec::new(), 0));
        }
        let flags = filter.flags_on();

        // No flag set: the scope's own ORDERPATH answers it directly,
        // and its total is an index count rather than a walk.
        let Some((key_flag, rest)) = flags.split_first() else {
            return match scope {
                Scope::All => self.list_default_via_table(user, filter, offset, limit),
                Scope::Bucket(b) => self.list_bucket_via_table(user, b, filter, offset, limit),
                Scope::Np => self.list_np_via_table(user, filter, offset, limit),
                Scope::Category(cat) => {
                    self.list_category_via_table(user, &cat, filter, offset, limit)
                }
                Scope::Contradiction => unreachable!("returned above"),
            };
        };

        // At least one flag: key on the first, and hand the engine every
        // other predicate as a value filter. Each flag index stores user
        // and activity beside the flag, so any one of them can be the
        // key and the rest cost a comparison — which is why "starred and
        // unread, within Inbox" needs no index of its own.
        let mut extra: Vec<(&str, String)> = rest.iter().map(|f| (*f, "1".to_string())).collect();
        match scope {
            Scope::All => {}
            Scope::Bucket(b) => {
                extra.push(("bucket", b.to_string()));
                if b == "inbox" {
                    // Inbox excludes threads that are nothing but the
                    // user's own messages — the same exclusion the
                    // bucket ORDERPATH carries in its sort prefix.
                    extra.push(("sent_only", "0".to_string()));
                }
            }
            Scope::Category(cat) => extra.push(("category", cat)),
            Scope::Np => {
                return self
                    .list_np_flagged_via_table(user, key_flag, &extra, filter, offset, limit);
            }
            Scope::Contradiction => unreachable!("returned above"),
        }
        self.list_stacked_via_table(user, key_flag, &extra, filter, offset, limit)
    }
}

mod paths;

#[cfg(test)]
mod tests;
