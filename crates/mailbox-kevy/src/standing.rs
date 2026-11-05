//! Where a conversation sits in one reader's lists: the `category` on their
//! membership row and the `bucket` (the tab) derived from it, the
//! `activity` every ORDERPATH sorts on and the list shows as the time, and
//! `requires_action`.
//!
//! They are the reader's, the way the SQL core aggregates them from the
//! reader's own messages: the category of their newest inbound message (a
//! reader's own move stamps every message of theirs, so it holds until mail
//! arrives for them), the newest inbound date (the newest of their own when
//! they sent every message), and whether any of their messages asks for
//! action. The shared thread hash keeps a category and a date too, from
//! whichever member's mail arrived or whichever member filed it last; the
//! membership row used to be copied from it, so one member's move put the
//! conversation in another member's Promotions tab and mail only one member
//! received re-sorted everybody's list. `thread_user_pairs` no longer emits
//! these fields, and [`write_standing`] is the only thing that writes them.
//!
//! The category each of the reader's messages was filed under is kept on
//! their own message row (`mailrs:usermsg:<user>:<mid>`), so a recount after
//! an expunge, a split or a merge can answer from the messages that are
//! left. Messages stored before that field existed answer from the
//! per-recipient placement record, or not at all, and then the row keeps
//! what it has.
//!
//! [`write_standing`]: KevyMailboxStore::write_standing

use std::io;

use super::KevyMailboxStore;
use super::keys;
use crate::thread_row::hset_membership;

const CATEGORY: &[u8] = b"category";
const BUCKET: &[u8] = b"bucket";
const ACTIVITY: &[u8] = b"activity";
const REQUIRES_ACTION: &[u8] = b"requires_action";
const SENT_ONLY: &[u8] = b"sent_only";
/// On the reader's message row: the category it was filed under for them.
const VERDICT: &[u8] = b"category";

/// One of the reader's messages, as much of it as the row needs.
struct Held {
    mid: String,
    date: i64,
    own: bool,
}

/// The row as it was before an arrival wrote anything.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Before {
    activity: Option<i64>,
    sent_only: bool,
}

/// What a recount found, for the correction route.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StandingRecount {
    /// The reader holds messages in the conversation; nothing else is
    /// recounted. Held messages with no known date leave the row as it is.
    pub held: bool,
    pub activity_from: i64,
    pub activity_to: i64,
    pub category_from: String,
    /// `None` when the reader's newest message has no recorded category,
    /// and the row keeps its own.
    pub category_to: Option<String>,
    /// The row differs from the reader's messages (and was written unless
    /// a dry run).
    pub differs: bool,
}

impl KevyMailboxStore {
    /// Record the category `user`'s copy of `message_id` was filed under.
    /// Only on an existing message row. Returns whether it wrote.
    pub fn set_message_category(
        &self,
        user: &str,
        message_id: &str,
        category: &str,
    ) -> io::Result<bool> {
        let key = keys::user_message(user, message_id);
        let wrote = self.store().atomic(|ctx| {
            if !ctx.hexists(key.as_bytes(), b"uid")? {
                return Ok(false);
            }
            if ctx.hget(key.as_bytes(), VERDICT)?.as_deref() == Some(category.as_bytes()) {
                return Ok(false);
            }
            ctx.hset(key.as_bytes(), &[(VERDICT, category.as_bytes())])?;
            Ok(true)
        })?;
        Ok(wrote)
    }

    /// The category `user`'s copy of `message_id` was filed under, or
    /// `None` when nothing recorded it.
    pub fn message_category(&self, user: &str, message_id: &str) -> io::Result<Option<String>> {
        let key = keys::user_message(user, message_id);
        if let Some(v) = self.store().hget(key.as_bytes(), VERDICT)? {
            return Ok(Some(String::from_utf8_lossy(&v).into_owned()));
        }
        // Older messages: the per-recipient placement record names the
        // classifier's or the reader's rule's class, and the reader's
        // filter's Junk. Mail left in the plain inbox has no record.
        let Some(json) = self.message_placement(user, message_id)? else {
            return Ok(None);
        };
        let reasons: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
        let mut class = None;
        for r in &reasons {
            match r["code"].as_str() {
                Some("junk-your-filter") => return Ok(Some("spam".into())),
                Some("sorted-by-classifier" | "sorted-by-your-rule") => {
                    class = r["params"]["class"].as_str().map(str::to_string);
                }
                _ => {}
            }
        }
        Ok(class)
    }

    fn held_dates(&self, user: &str, thread_id: &str) -> io::Result<Vec<Held>> {
        let key = keys::thread_user_messages(user, thread_id);
        let mut out = Vec::new();
        for (m, score) in self.store().zrange(key.as_bytes(), 0, -1)? {
            let Ok(mid) = String::from_utf8(m) else {
                continue;
            };
            let Some(facts) = self.user_message_facts(user, &mid)? else {
                continue;
            };
            out.push(Held {
                mid,
                date: score as i64,
                own: facts.own,
            });
        }
        Ok(out)
    }

    /// The reader's newest inbound message, or their newest when they sent
    /// every one.
    fn newest_of(held: &[Held]) -> Option<&Held> {
        // A date of 0 is not known (rows written by an old backfill), so
        // such a message cannot say what is newest. On a tie in date the
        // later in zset order, as `max_by_key` keeps.
        let newest = |own: bool| {
            held.iter()
                .filter(|h| h.own == own && h.date > 0)
                .max_by_key(|h| h.date)
        };
        newest(false).or_else(|| newest(true))
    }

    /// Activity and category from `user`'s own messages, or `None` when they
    /// hold none with a known date. The category is `None` when the newest has none recorded.
    fn standing_from(
        &self,
        user: &str,
        held: &[Held],
    ) -> io::Result<Option<(i64, Option<String>)>> {
        let Some(newest) = Self::newest_of(held) else {
            return Ok(None);
        };
        Ok(Some((
            newest.date,
            self.message_category(user, &newest.mid)?,
        )))
    }

    /// `user`'s stored category and activity, or `None` when they have no
    /// row.
    fn stored_standing(&self, user: &str, thread_id: &str) -> io::Result<Option<(String, i64)>> {
        let key = keys::thread_user(user, thread_id);
        let store = self.store();
        if !store.hexists(key.as_bytes(), b"tid")? {
            return Ok(None);
        }
        let category = store
            .hget(key.as_bytes(), CATEGORY)?
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default();
        let activity = store
            .hget(key.as_bytes(), ACTIVITY)?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(Some((category, activity)))
    }

    /// Put `user`'s category and activity for `thread_id` back to what their
    /// own messages say. A row with no messages of theirs is left alone, and
    /// so is a category their newest message has none recorded for.
    pub fn recount_standing(
        &self,
        user: &str,
        thread_id: &str,
        dry_run: bool,
    ) -> io::Result<StandingRecount> {
        let Some((category_from, activity_from)) = self.stored_standing(user, thread_id)? else {
            return Ok(StandingRecount::default());
        };
        let held = self.held_dates(user, thread_id)?;
        let Some((activity_to, category_to)) = self.standing_from(user, &held)? else {
            return Ok(StandingRecount {
                held: !held.is_empty(),
                category_from,
                activity_from,
                activity_to: activity_from,
                ..Default::default()
            });
        };
        let differs = activity_to != activity_from
            || category_to.as_ref().is_some_and(|c| *c != category_from);
        if differs && !dry_run {
            self.write_standing(
                user,
                thread_id,
                category_to.as_deref(),
                Some(activity_to),
                None,
            )?;
        }
        Ok(StandingRecount {
            held: true,
            activity_from,
            activity_to,
            category_from,
            category_to,
            differs,
        })
    }

    /// The row's activity and whether it held only the reader's own mail,
    /// read before an arrival changes either.
    pub(crate) fn standing_before(&self, user: &str, thread_id: &str) -> io::Result<Before> {
        let key = keys::thread_user(user, thread_id);
        let store = self.store();
        let activity = match store.hexists(key.as_bytes(), b"tid")? {
            true => store
                .hget(key.as_bytes(), ACTIVITY)?
                .and_then(|v| String::from_utf8(v).ok())
                .and_then(|s| s.parse().ok()),
            false => None,
        };
        let sent_only = store.hget(key.as_bytes(), SENT_ONLY)?.as_deref() == Some(b"1");
        Ok(Before {
            activity,
            sent_only,
        })
    }

    /// An arriving message onto `user`'s row: mail from somebody else
    /// carries the conversation to its date and its category unless it is
    /// no newer than what the row already shows; the reader's own moves the
    /// date only while the conversation holds nothing else.
    pub(crate) fn arrive_standing(
        &self,
        m: &crate::MessageArrival<'_>,
        before: Before,
    ) -> io::Result<bool> {
        let (activity, category) = match (m.is_own, before.activity) {
            (_, None) => (Some(m.latest_date), Some(m.category)),
            (false, Some(_)) if before.sent_only => (Some(m.latest_date), Some(m.category)),
            // strictly newer: a message announced again (the maildir heal)
            // is not new mail and does not undo the reader's own move
            (false, Some(have)) if m.latest_date > have => (Some(m.latest_date), Some(m.category)),
            (true, Some(have)) if before.sent_only => (Some(have.max(m.latest_date)), None),
            _ => (None, None),
        };
        self.write_standing(m.user, m.thread_id, category, activity, None)
    }

    /// A reader's own move: the row and every message of theirs in the
    /// conversation, so the move holds until mail arrives for them, as the
    /// SQL core stamps each of the reader's messages.
    pub(crate) fn file_for_reader(
        &self,
        user: &str,
        thread_id: &str,
        category: &str,
    ) -> io::Result<bool> {
        for mid in self.user_thread_message_ids(user, thread_id)? {
            self.set_message_category(user, &mid, category)?;
        }
        self.write_standing(user, thread_id, Some(category), None, None)
    }

    /// The fields a membership row created by a write from a
    /// [`crate::ThreadRow`] starts with: the reader's own messages decide
    /// when they hold any, and the caller's values stand only for what those
    /// cannot answer (an import that writes the row before its messages, a
    /// fixture).
    pub(crate) fn seed_standing(
        &self,
        user: &str,
        row: &crate::ThreadRow,
    ) -> io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let held = self.held_dates(user, &row.thread_id)?;
        let (activity, category) = match self.standing_from(user, &held)? {
            Some((a, c)) => (a, c.unwrap_or_else(|| row.category.clone())),
            None => (row.latest_date, row.category.clone()),
        };
        Ok(vec![
            (
                BUCKET.to_vec(),
                keys::bucket_of(&category).name().as_bytes().to_vec(),
            ),
            (CATEGORY.to_vec(), category.into_bytes()),
            (ACTIVITY.to_vec(), activity.to_string().into_bytes()),
            (
                REQUIRES_ACTION.to_vec(),
                crate::thread_row::flag(row.requires_action).to_vec(),
            ),
        ])
    }

    /// Set `user`'s `requires_action`, theirs alone.
    pub fn set_requires_action(&self, user: &str, thread_id: &str, on: bool) -> io::Result<bool> {
        self.write_standing(user, thread_id, None, None, Some(on))
    }

    /// The one writer of the four fields. Only on an existing row, so it
    /// cannot conjure membership, and only what changes; `bucket` always
    /// follows `category`. Returns whether it wrote.
    pub(crate) fn write_standing(
        &self,
        user: &str,
        thread_id: &str,
        category: Option<&str>,
        activity: Option<i64>,
        requires_action: Option<bool>,
    ) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        let activity = activity.map(|a| a.to_string());
        let bucket = category.map(|c| keys::bucket_of(c).name());
        let mut want: Vec<(&[u8], &[u8])> = Vec::new();
        if let (Some(c), Some(b)) = (category, bucket) {
            want.push((CATEGORY, c.as_bytes()));
            want.push((BUCKET, b.as_bytes()));
        }
        if let Some(a) = &activity {
            want.push((ACTIVITY, a.as_bytes()));
        }
        if let Some(on) = requires_action {
            want.push((REQUIRES_ACTION, crate::thread_row::flag(on)));
        }
        let (wrote, rebucketed) = self.store().atomic(|ctx| {
            if !ctx.hexists(key.as_bytes(), b"tid")? {
                return Ok((false, false));
            }
            let mut changed: Vec<(&[u8], &[u8])> = Vec::new();
            for (f, v) in &want {
                if ctx.hget(key.as_bytes(), f)?.as_deref() != Some(*v) {
                    changed.push((f, v));
                }
            }
            if changed.is_empty() {
                return Ok((false, false));
            }
            let rebucketed = changed.iter().any(|(f, _)| *f == BUCKET);
            hset_membership(ctx, key.as_bytes(), &changed)?;
            Ok((true, rebucketed))
        })?;
        // in or out of Junk changes whether it has a section
        if rebucketed {
            self.rewrite_section(&key)?;
        }
        Ok(wrote)
    }
}
