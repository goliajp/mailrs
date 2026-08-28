//! The membership row: one (user, thread) pair, and the columns a
//! declared index keys on.
//!
//! Split out of `mod.rs` by theme rather than by line count — this is
//! the half that answers "which of my lists is this conversation in",
//! and the parent is the shared thread's own shape. A submodule
//! because it reads the parent's types; the other direction would
//! have needed `pub(crate)` on things that are nobody else's business.

use super::*;

/// The declared columns that are one user's state rather than the
/// conversation's, and so are never derived from the shared thread hash.
///
/// Each is written by its own mutator against the membership row.
/// `thread_user_pairs` leaves them alone; a fresh row gets them at zero.
pub(crate) const PER_USER_FLAGS: [&str; 7] = [
    "starred",
    "archived",
    // A finding about this reader's mail, so it is theirs and not the
    // shared thread's: two people on one conversation can have been
    // targeted differently, and one of them releasing it must not
    // release it for the other.
    //
    // Like `archived` it is an equality component of every ORDERPATH
    // prefix, so a row missing it is in **no** list rather than merely
    // un-quarantined — which is why it is planted at zero rather than
    // left absent.
    "quarantined",
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

/// The two declared axis columns, from a thread's total and the number
/// of its messages this user sent.
///
/// **One definition of each**, because both decide which list a thread
/// appears in: the Inbox ORDERPATH excludes `sent_only` and the Sent
/// axis keys on `is_sender`.
///
/// "sent_only" means every message in the thread came from this user —
/// it lives in Sent and nowhere else. Merely having replied does not
/// qualify: a conversation the user took part in is still an inbox
/// thread, and reading it as "has ever sent" dropped 190 threads from
/// one account's inbox on production.
///
/// `is_sender` is the other half of that distinction: the Sent folder
/// shows every thread the user has written in, the way Gmail does. A
/// conversation they replied in is in both.
pub(crate) fn axis_flags(total: i64, own: i64) -> (bool, bool) {
    (total > 0 && own >= total, own > 0)
}

/// `counts` is `(total, own)` **for this user**, from the declared
/// index. `None` when the index cannot answer — a thread whose rows
/// predate the group column — and then the shared row's numbers stand in.
///
/// The fallback is the old behaviour and is deliberately kept: those
/// counters are everybody's on a multi-owner thread, so one owner's
/// replies can set another's `sent_only`. Measured on a copy of
/// production over 32,206 threads and 159 multi-owner ones, that has
/// never happened — but the hazard is in the code, and the engine's
/// answer removes it wherever the index can speak.
pub(crate) fn thread_user_pairs(
    user: &str,
    row: &ThreadRow,
    counts: Option<(i64, i64)>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let bucket = keys::bucket_of(&row.category);
    let (sent_only, is_sender) = match counts {
        Some((total, own)) => axis_flags(total, own),
        // The fallback, for a thread the index cannot answer for — one
        // with no per-user message rows yet.
        //
        // It used to read `row.count > 0 && row.sent_count >= row.count`.
        // Those fields are no longer written (C5b-2), so that expression
        // became a constant `false`, and a thread the user had only ever
        // sent in would have appeared in their Inbox. A test caught it;
        // production never could have, because every row there is
        // backfilled and the index answers for all of them.
        //
        // `senders_csv` is the only per-thread evidence left on the row,
        // and it answers both questions directly: *only* this user wrote
        // in it, versus this user is *among* those who did.
        //
        // It is a display string that accumulates, so it can say the user
        // wrote in a thread they only received — one such row on
        // production, a message from noreply@ addressed to them. That is
        // why the index's answer is preferred wherever it exists.
        None => (
            mailrs_rfc5322::list_is_only(&row.senders_csv, user),
            senders_csv_contains_user(&row.senders_csv, user),
        ),
    };
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
        (b"account_id".to_vec(), row.account_id.as_bytes().to_vec()),
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
