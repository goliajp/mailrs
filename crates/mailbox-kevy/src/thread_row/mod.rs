//! What a thread row *is* — the shape both the shared conversation hash
//! and each owner's membership row are read from and written as.
//!
//! The store operations that use it live in [`store`]; keeping the shape
//! here means the two directions of every conversion sit next to each
//! other, which is the pairing that has to stay honest
//! (`field_names_match_to_pairs`).

use super::keys;

/// Sent-folder membership predicate — true when the thread's
/// `senders_csv` names the user's own address.
///
/// Used by `upsert_thread` (write path), by the backfill, and through
/// `thread_user_pairs` it decides the declared `is_sender` column, which is
/// the Sent axis itself.
///
/// Whole-address comparison, via the one stone that knows how to reduce an
/// RFC 5322 mailbox to a comparison key. This matched by **substring**
/// until 2026-07-30, so any address ending with the user's own put a
/// foreign thread in their Sent folder — `a@b.com` matched `xa@b.com`.
/// A ninth hand-rolled address extractor was the alternative; the stone
/// exists so there is no tenth.
pub fn senders_csv_contains_user(senders_csv: &str, user: &str) -> bool {
    mailrs_rfc5322::list_contains(senders_csv, user)
}

/// The message a thread's row should describe: the newest one the user
/// did not send.
///
/// The row follows the last **inbound** message (2026-07-18). A reply
/// is the user telling themselves something they already know; letting
/// it re-date the row moves a conversation to the top of Inbox for no
/// arrival, and re-titling it replaces the correspondent's subject with
/// the user's own.
///
/// A thread of nothing but the user's own sends still needs a date and
/// a subject, so with no inbound message the newest own one is used —
/// which is what makes a sent-only thread show its send time rather
/// than 1970.
///
/// `wires` is the thread's messages in date order, as
/// `thread_messages_unscoped` returns them.
pub fn display_message(wires: &[Vec<u8>], user: &str) -> Option<serde_json::Value> {
    let parsed: Vec<serde_json::Value> = wires
        .iter()
        .filter_map(|w| serde_json::from_slice(w).ok())
        .collect();
    // Searched from the back: the newest inbound message is the last
    // one that matches, and scanning a long conversation forwards to
    // find it is work for nothing.
    let inbound = parsed
        .iter()
        .rfind(|w| !senders_csv_contains_user(w["sender"].as_str().unwrap_or(""), user))
        .cloned();
    inbound.or_else(|| parsed.last().cloned())
}

/// Aggregated thread state — one row in `mailrs:thread:<tid>`.
///
/// Stable on-the-wire field names: the kevy hash uses these exact
/// byte strings as field keys so a future debug dump (kevy-cli HGETALL)
/// stays readable.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadRow {
    pub thread_id: String,
    pub subject: String,
    pub senders_csv: String,
    pub count: i64,
    pub unread_count: i64,
    pub latest_date: i64,
    pub latest_preview: String,
    pub category: String,
    pub importance_level: String,
    pub importance_score: f64,
    pub requires_action: bool,
    pub pinned: bool,
    pub archived: bool,
    pub has_action: bool,
    pub sent_count: i64,
    pub starred: bool,
    /// Epoch seconds this reader put the thread away until, or 0.
    ///
    /// Per reader, like `starred` and `archived` beside it — a snooze
    /// used to be written to the shared thread hash, where putting a
    /// conversation away would have done it for everyone who could
    /// see it, if anything had read the field at all.
    pub snoozed_until: i64,
}

impl ThreadRow {
    /// Every field name a thread row writes. `delete_thread` deletes
    /// exactly this set — kevy has no HCLEAR, so the list has to be
    /// spelled out, and a field missing from it leaves the row
    /// half-alive after a delete. Adding `search_blob` without updating
    /// the delete list did exactly that, and would have left deleted
    /// mail sitting in the search index (caught by
    /// `delete_thread_clears_all_indexes`, 2026-07-19).
    ///
    /// `field_names_match_to_pairs` keeps this honest.
    pub(crate) fn field_names() -> &'static [&'static [u8]] {
        &[
            b"search_blob",
            b"subject",
            b"senders_csv",
            b"count",
            b"unread_count",
            b"latest_date",
            b"latest_preview",
            b"category",
            b"importance_level",
            b"importance_score",
            b"requires_action",
            b"pinned",
            b"archived",
            b"has_action",
            b"sent_count",
            b"starred",
        ]
    }

    fn to_pairs(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        macro_rules! kv {
            ($k:expr, $v:expr) => {
                ($k.as_bytes().to_vec(), $v.into_bytes())
            };
        }
        vec![
            kv!(
                "search_blob",
                keys::search_blob(&self.subject, &self.senders_csv, &self.latest_preview)
            ),
            kv!("subject", self.subject.clone()),
            kv!("senders_csv", self.senders_csv.clone()),
            kv!("count", self.count.to_string()),
            kv!("unread_count", self.unread_count.to_string()),
            kv!("latest_date", self.latest_date.to_string()),
            kv!("latest_preview", self.latest_preview.clone()),
            kv!("category", self.category.clone()),
            kv!("importance_level", self.importance_level.clone()),
            kv!("importance_score", self.importance_score.to_string()),
            kv!("requires_action", (self.requires_action as u8).to_string()),
            kv!("sent_count", self.sent_count.to_string()),
            // `pinned`, `archived`, `has_action` and `starred` are not
            // written here any more: they are one user's state and they
            // live on that user's membership row. What the shared hash
            // still holds is what a conversation is — who wrote in it,
            // when, what it says, how it was classified.
            //
            // They stay in `field_names()`, which is the delete list, so
            // `delete_thread` still clears the values written before
            // this.
        ]
    }

    pub(crate) fn from_pairs(thread_id: String, pairs: &[(Vec<u8>, Vec<u8>)]) -> Option<Self> {
        if pairs.is_empty() {
            return None;
        }
        let mut subject = String::new();
        let mut senders_csv = String::new();
        let mut count = 0;
        let mut unread_count = 0;
        let mut latest_date = 0;
        let mut latest_preview = String::new();
        let mut category = String::new();
        let mut importance_level = String::new();
        let mut importance_score = 0.0;
        let mut requires_action = false;
        let mut pinned = false;
        let mut archived = false;
        let mut has_action = false;
        let mut sent_count = 0;
        let mut starred = false;
        for (k, v) in pairs {
            let kk = std::str::from_utf8(k).ok()?;
            let vv = std::str::from_utf8(v).ok()?;
            match kk {
                "subject" => subject = vv.into(),
                "senders_csv" => senders_csv = vv.into(),
                "count" => count = vv.parse().unwrap_or(0),
                "unread_count" => unread_count = vv.parse().unwrap_or(0),
                "latest_date" => latest_date = vv.parse().unwrap_or(0),
                "latest_preview" => latest_preview = vv.into(),
                "category" => category = vv.into(),
                "importance_level" => importance_level = vv.into(),
                "importance_score" => importance_score = vv.parse().unwrap_or(0.0),
                "requires_action" => requires_action = vv == "1",
                "pinned" => pinned = vv == "1",
                "archived" => archived = vv == "1",
                "has_action" => has_action = vv == "1",
                "sent_count" => sent_count = vv.parse().unwrap_or(0),
                "starred" => starred = vv == "1",
                _ => {}
            }
        }
        Some(Self {
            thread_id,
            subject,
            senders_csv,
            count,
            unread_count,
            latest_date,
            latest_preview,
            category,
            importance_level,
            importance_score,
            requires_action,
            pinned,
            archived,
            has_action,
            sent_count,
            starred,
            // The shared hash, which has no user segment: a snooze is
            // one reader's, and lives on their membership row. Reading
            // it here would be reading somebody's else's.
            snoozed_until: 0,
        })
    }
}

impl ThreadRow {
    /// Build a row from a **membership row**'s fields — this user's copy
    /// of the conversation, rather than the shared aggregate.
    ///
    /// The two hashes name the same facts differently in two places:
    /// `activity` is the membership row's `latest_date`, and the flags
    /// are stored as the declared `1`/`0` columns the indexes read. The
    /// counters are per-user and maintained by `hincrby` on the arrival
    /// path, so they are read straight off this row.
    ///
    /// Returns `None` when the row is absent — the user has no copy —
    /// which is the same answer `user_message_view` gives for a message.
    pub fn from_user_pairs(thread_id: String, pairs: &[(Vec<u8>, Vec<u8>)]) -> Option<Self> {
        if pairs.is_empty() {
            return None;
        }
        let mut row = Self {
            thread_id,
            subject: String::new(),
            senders_csv: String::new(),
            count: 0,
            unread_count: 0,
            latest_date: 0,
            latest_preview: String::new(),
            category: String::new(),
            importance_level: String::new(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
            snoozed_until: 0,
        };
        for (k, v) in pairs {
            let (Ok(kk), Ok(vv)) = (std::str::from_utf8(k), std::str::from_utf8(v)) else {
                continue;
            };
            match kk {
                "subject" => row.subject = vv.into(),
                "senders_csv" => row.senders_csv = vv.into(),
                "count" => row.count = vv.parse().unwrap_or(0),
                "unread_count" => row.unread_count = vv.parse().unwrap_or(0),
                "sent_count" => row.sent_count = vv.parse().unwrap_or(0),
                // The membership row calls it `activity`: it is the
                // column every ORDERPATH sorts on, so the name says what
                // it is for rather than where it came from.
                "activity" => row.latest_date = vv.parse().unwrap_or(0),
                "latest_preview" => row.latest_preview = vv.into(),
                "category" => row.category = vv.into(),
                "importance_level" => row.importance_level = vv.into(),
                "importance_score" => row.importance_score = vv.parse().unwrap_or(0.0),
                "requires_action" => row.requires_action = vv == "1",
                "pinned" => row.pinned = vv == "1",
                "archived" => row.archived = vv == "1",
                "has_action" => row.has_action = vv == "1",
                "starred" => row.starred = vv == "1",
                "snoozed_until" => row.snoozed_until = vv.parse().unwrap_or(0),
                _ => {}
            }
        }
        Some(row)
    }
}

/// The declared columns that are one user's state rather than the
/// conversation's, and so are never derived from the shared thread hash.
///
/// Each is written by its own mutator against the membership row.
/// `thread_user_pairs` leaves them alone; a fresh row gets them at zero.
pub(crate) const PER_USER_FLAGS: [&str; 6] = [
    "starred",
    "archived",
    "pinned",
    "unread",
    "has_action",
    // Not a flag, but planted with them for the same reason: a
    // `FILTER snoozed_until <= now` drops every row that does not
    // carry the field at all, so a row without it would vanish from
    // the inbox rather than stay in it.
    "snoozed_until",
];

/// `1` / `0` as the stored bytes for a boolean column. i64-typed in the
/// declaration so `FILTER flag EQ 1` coerces cleanly.
pub(crate) fn flag(v: bool) -> &'static [u8] {
    if v { b"1" } else { b"0" }
}

/// The membership-row fields for one (user, thread) pair.
///
/// Shared by the live write path and the backfill so the two cannot
/// disagree about what a row contains — a drift between "what writes
/// put there" and "what backfill puts there" would be exactly the
/// class of bug this whole migration exists to remove.
/// A bounded tie-breaker derived from the thread id.
///
/// The id is a Message-ID and can exceed kevy's `MAX_STR_COMPONENT`
/// (255 bytes), and a composite orderpath **excludes the whole row**
/// when any component is over that — two threads on prod disappeared
/// from both composites that way. FNV-1a folded into a non-negative
/// i64 is always in range, so the sort stays total without any row
/// being able to fall out of it.
fn tid_ord(tid: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tid.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h >> 1) as i64
}

pub(crate) fn thread_user_pairs(user: &str, row: &ThreadRow) -> Vec<(Vec<u8>, Vec<u8>)> {
    let bucket = keys::bucket_of(&row.category);
    // "sent_only" means every message in the thread came from this
    // user — it lives in Sent and nowhere else. Merely having replied
    // does not qualify: a conversation the user took part in is still
    // an inbox thread. Reading it as "has ever sent" dropped 190
    // threads from one account's inbox on prod.
    let sent_only = row.count > 0 && row.sent_count >= row.count;
    // Distinct from `sent_only`: the Sent folder shows every thread
    // the user has written in, the way Gmail does, while the inbox
    // only excludes threads that are *nothing but* their own messages.
    // A conversation they replied in is in both.
    let is_sender = senders_csv_contains_user(&row.senders_csv, user);
    vec![
        (b"user".to_vec(), user.as_bytes().to_vec()),
        (b"tid".to_vec(), row.thread_id.as_bytes().to_vec()),
        (
            b"ord".to_vec(),
            tid_ord(&row.thread_id).to_string().into_bytes(),
        ),
        (b"bucket".to_vec(), bucket.name().as_bytes().to_vec()),
        (b"category".to_vec(), row.category.as_bytes().to_vec()),
        (
            b"activity".to_vec(),
            row.latest_date.to_string().into_bytes(),
        ),
        (b"sent_only".to_vec(), flag(sent_only).to_vec()),
        (b"is_sender".to_vec(), flag(is_sender).to_vec()),
        // `starred`, `archived`, `pinned`, `has_action` and `unread` are
        // **not** here, and that is the point of the row.
        //
        // They are one person's state, and this function derives from the
        // shared thread hash, which has no user segment. Emitting them
        // meant every arrival rewrote each owner's flags with whatever the
        // last owner had set: A stars a conversation, mail arrives for B,
        // and B's row is now starred too — silently, with nothing to
        // compare against. `keys.rs` states the rule this broke: "every
        // per-user fact belongs on a row of its own".
        //
        // Each has a writer that already targets the membership row —
        // `toggle_flag`, `mark_seen`, `mark_unread`,
        // `record_message_arrival` for `unread` — so leaving them out
        // removes a write rather than losing one.
        // [`KevyMailboxStore::plant_thread_user_defaults`] gives a row its
        // first zeros so the declared columns exist from the start.
        // Display payload, so a list page can be served from this row
        // alone instead of joining back to the shared thread hash
        // (RFC 20260730 S1). Undeclared by the TableSpec — nothing
        // indexes or sorts on them — so adding them does not change the
        // spec and does not rebuild 30k rows' indexes at boot.
        //
        // `latest_date` is absent because it is already here as
        // `activity`, and `category` because it is already a column.
        //
        // The three counters are absent for a different reason: they
        // are maintained per user by `hincrby` on the arrival path, and
        // this list is written with `hset`, which would overwrite each
        // increment with the shared row's total.
        (b"subject".to_vec(), row.subject.as_bytes().to_vec()),
        (b"senders_csv".to_vec(), row.senders_csv.as_bytes().to_vec()),
        (
            b"latest_preview".to_vec(),
            row.latest_preview.as_bytes().to_vec(),
        ),
        (
            b"importance_level".to_vec(),
            row.importance_level.as_bytes().to_vec(),
        ),
        (
            b"importance_score".to_vec(),
            row.importance_score.to_string().into_bytes(),
        ),
        (
            b"requires_action".to_vec(),
            flag(row.requires_action).to_vec(),
        ),
    ]
}
mod store;

#[cfg(test)]
mod tests {
    /// `is_sender` is the Sent axis, and this predicate decides it. The
    /// substring form it replaced put a foreign thread in an account's Sent
    /// folder whenever some other address merely ended with the user's own.
    #[test]
    fn sent_membership_needs_the_whole_address() {
        let u = "lihao@golia.jp";
        assert!(senders_csv_contains_user("lihao@golia.jp", u));
        // The form prod actually stores in `senders_csv`.
        assert!(senders_csv_contains_user(
            "GOLIA <lihao@golia.jp>, x@y.com",
            u
        ));
        assert!(senders_csv_contains_user("LiHao@Golia.JP", u));

        assert!(
            !senders_csv_contains_user("notlihao@golia.jp", u),
            "a longer local part is a different mailbox"
        );
        assert!(
            !senders_csv_contains_user("lihao@golia.jp.evil.example", u),
            "a longer domain is a different mailbox"
        );
        assert!(!senders_csv_contains_user("", u));
    }

    use super::*;
    use crate::KevyMailboxStore;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        s
    }

    #[test]
    fn membership_backfill_converges() {
        // `periodic-work-must-converge`: the second pass over data that
        // is already right must write nothing. An unconditional hset
        // would be idempotent and still churn the AOF forever.
        let st = store();
        let row = sample("t-converge");
        assert!(
            st.write_thread_user_if_changed("alice@x.com", &row)
                .unwrap(),
            "first write must create the row"
        );
        assert!(
            !st.write_thread_user_if_changed("alice@x.com", &row)
                .unwrap(),
            "second write over identical data must be a no-op"
        );

        let mut moved = row.clone();
        moved.category = "spam".into();
        assert!(
            st.write_thread_user_if_changed("alice@x.com", &moved)
                .unwrap(),
            "a changed field must write again"
        );
    }

    /// Every declared per-user flag exists on the row, whatever wrote it
    /// first.
    ///
    /// `archived` is an equality component of every ORDERPATH prefix, so
    /// a row without it is in none of them — invisible in every list
    /// rather than merely un-archived. The arrival path writes this
    /// user's counters to the row before the row-writer runs, so the row
    /// is not empty by the time planting was asked whether it was new,
    /// and the flags were never planted at all on the one path that
    /// creates most rows.
    #[test]
    fn a_row_created_by_an_arrival_carries_every_declared_flag() {
        let st = store();
        let u = "alice@x.com";
        st.record_message_arrival(&crate::MessageArrival {
            thread_id: "t-arrival",
            user: u,
            subject: "Hello",
            senders_csv: "bob@y.com",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();

        let have: std::collections::HashMap<Vec<u8>, Vec<u8>> = st
            .store()
            .hgetall(keys::thread_user(u, "t-arrival").as_bytes())
            .unwrap()
            .into_iter()
            .collect();
        let absent: Vec<&str> = PER_USER_FLAGS
            .iter()
            .copied()
            .filter(|f| !have.contains_key(f.as_bytes()))
            .collect();
        assert!(absent.is_empty(), "declared flags missing: {absent:?}");
        assert_eq!(
            have.get(b"archived".as_slice()).map(Vec::as_slice),
            Some(b"0".as_slice())
        );
        assert_eq!(
            have.get(b"unread".as_slice()).map(Vec::as_slice),
            Some(b"1".as_slice()),
            "and planting must not overwrite the flag the arrival set"
        );
    }

    #[test]
    fn field_names_match_to_pairs() {
        // The delete path deletes `field_names()`; the write path writes
        // `to_pairs()`. A field written and not deleted leaves a
        // partially deleted row behind, so the delete list has to cover
        // the write list.
        //
        // The reverse does not hold, and deliberately: the four
        // user-curated flags moved to the membership row and are no
        // longer written here, but rows written before that still carry
        // them and a delete has to take them with it.
        let written: std::collections::BTreeSet<Vec<u8>> =
            sample("t").to_pairs().into_iter().map(|(k, _)| k).collect();
        let declared: std::collections::BTreeSet<Vec<u8>> = ThreadRow::field_names()
            .iter()
            .map(|f| f.to_vec())
            .collect();
        let unlisted: Vec<String> = written
            .difference(&declared)
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .collect();
        assert!(
            unlisted.is_empty(),
            "to_pairs() writes fields delete_thread never clears: {unlisted:?}"
        );
        let legacy: Vec<String> = declared
            .difference(&written)
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .collect();
        assert_eq!(
            legacy,
            vec![
                "archived".to_string(),
                "has_action".to_string(),
                "pinned".to_string(),
                "starred".to_string()
            ],
            "the only unwritten entries are the flags that moved to the \
             membership row; anything else is drift"
        );
    }

    fn sample(tid: &str) -> ThreadRow {
        ThreadRow {
            thread_id: tid.into(),
            subject: "Hello".into(),
            senders_csv: "alice@x.com,bob@y.com".into(),
            count: 3,
            unread_count: 1,
            latest_date: 1782846047,
            latest_preview: "OTP is 881576".into(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.5,
            requires_action: false,
            pinned: true,
            archived: false,
            has_action: true,
            sent_count: 1,
            starred: false,
            snoozed_until: 0,
        }
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let s = store();
        let row = sample("t1");
        s.upsert_thread("u@x.com", &row).unwrap();
        let back = s.get_thread("t1").unwrap().unwrap();
        // Everything the shared hash is still the authority for. The
        // four user-curated flags are not among them — they answer
        // "whose?", which this hash cannot — so they come back off.
        assert_eq!(
            back,
            ThreadRow {
                pinned: false,
                archived: false,
                has_action: false,
                starred: false,
                ..row
            }
        );
    }

    /// And they round-trip through the row that *can* answer "whose".
    #[test]
    fn the_users_own_row_round_trips_the_flags() {
        let s = store();
        s.upsert_thread("u@x.com", &sample("t1")).unwrap();
        s.set_starred("u@x.com", "t1", true).unwrap();
        s.set_pinned("u@x.com", "t1", true).unwrap();

        let mine = s
            .get_thread_for_user("u@x.com", "t1")
            .unwrap()
            .expect("membership row");
        assert!(mine.starred);
        assert!(mine.pinned);
        assert!(!mine.archived);
        assert_eq!(mine.subject, sample("t1").subject);
    }

    #[test]
    fn get_missing_returns_none() {
        let s = store();
        assert!(s.get_thread("nope").unwrap().is_none());
    }

    #[test]
    fn pinned_archived_flags_toggle_membership_row() {
        let s = store();
        let field = |tid: &str, name: &str| -> String {
            s.store()
                .hgetall(keys::thread_user("u@x.com", tid).as_bytes())
                .unwrap()
                .into_iter()
                .find(|(f, _)| f == name.as_bytes())
                .map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default()
        };

        // A fresh row starts with every per-user flag off, whatever the
        // aggregate says — a conversation you have just received is not
        // archived *by you*.
        let mut row = sample("t2");
        row.archived = true;
        row.pinned = true;
        s.upsert_thread("u@x.com", &row).unwrap();
        assert_eq!(field("t2", "archived"), "0");
        assert_eq!(field("t2", "pinned"), "0");

        // The mutators own them.
        s.set_archived("u@x.com", "t2", true).unwrap();
        s.set_pinned("u@x.com", "t2", true).unwrap();
        assert_eq!(field("t2", "archived"), "1");
        assert_eq!(field("t2", "pinned"), "1");

        // And a later arrival does not undo the user's own decision,
        // which is what deriving these from the shared hash did.
        row.archived = false;
        row.pinned = false;
        s.upsert_thread("u@x.com", &row).unwrap();
        assert_eq!(field("t2", "archived"), "1");
        assert_eq!(field("t2", "pinned"), "1");
    }

    /// The defect this migration exists for: two accounts on one thread,
    /// one of them stars it, mail arrives — and the other's row is
    /// untouched.
    ///
    /// 74 threads on production have two owners. Before this, the shared
    /// hash carried `starred`, `thread_user_pairs` copied it onto every
    /// owner's row on every arrival, and nothing could tell you it had
    /// happened.
    #[test]
    fn one_owners_star_does_not_reach_the_others_row() {
        let s = store();
        let row = sample("shared-tid");
        s.upsert_thread("a@x.com", &row).unwrap();
        s.upsert_thread("b@x.com", &row).unwrap();

        s.set_starred("a@x.com", "shared-tid", true).unwrap();

        // A new message in the conversation, delivered to both.
        for u in ["a@x.com", "b@x.com"] {
            s.upsert_thread(u, &row).unwrap();
        }

        assert!(
            s.get_thread_for_user("a@x.com", "shared-tid")
                .unwrap()
                .expect("a's row")
                .starred,
            "the owner who starred it keeps it"
        );
        assert!(
            !s.get_thread_for_user("b@x.com", "shared-tid")
                .unwrap()
                .expect("b's row")
                .starred,
            "the other owner never starred anything"
        );
    }

    #[test]
    fn membership_row_carries_latest_date_as_activity() {
        let s = store();
        let row = sample("t3");
        s.upsert_thread("u@x.com", &row).unwrap();
        let activity = s
            .store()
            .hgetall(keys::thread_user("u@x.com", "t3").as_bytes())
            .unwrap()
            .into_iter()
            .find(|(f, _)| f == b"activity")
            .map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default();
        assert_eq!(activity, row.latest_date.to_string());
    }
}
