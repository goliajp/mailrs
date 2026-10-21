//! UID reservation: one sequence per user, shared by all of the user's
//! mailboxes.
//!
//! The core-api names a message by `(user, uid)` alone, so a UID must not
//! repeat between two of a user's mailboxes. IMAP only needs it unique and
//! ascending within one mailbox, which a per-user sequence also gives;
//! each mailbox's `uidnext` stays above every UID it holds.

use crate::pg::BackendDb;

type Conn = <BackendDb as sqlx::Database>::Connection;

/// A block of `n` consecutive UIDs reserved in one mailbox, and that
/// mailbox's modseq advanced by `n`.
pub(super) struct Reserved {
    pub mailbox_id: i64,
    pub first_uid: i32,
    /// The modseq before the reservation; the k-th message gets
    /// `base_modseq + k + 1`.
    pub base_modseq: i64,
}

pub(super) async fn reserve(
    conn: &mut Conn,
    user: &str,
    mailbox_name: &str,
    n: i32,
) -> Result<Reserved, sqlx::Error> {
    // plain columns only in RETURNING: spg types arithmetic expressions
    // there as TEXT
    let (last_uid,): (i32,) = sqlx::query_as(
        "INSERT INTO user_uids (user_address, last_uid) VALUES ($1, $2)
         ON CONFLICT (user_address) DO UPDATE SET last_uid = user_uids.last_uid + $2
         RETURNING last_uid",
    )
    .bind(user)
    .bind(n)
    .fetch_one(&mut *conn)
    .await?;
    let first_uid = last_uid - n + 1;
    let (mailbox_id, new_modseq): (i64, i64) = sqlx::query_as(
        "UPDATE mailboxes
         SET uidnext = CASE WHEN uidnext > $1 THEN uidnext ELSE $1 END,
             highest_modseq = highest_modseq + $2
         WHERE user_address = $3 AND name = $4
         RETURNING id, highest_modseq",
    )
    .bind(last_uid + 1)
    .bind(i64::from(n))
    .bind(user)
    .bind(mailbox_name)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Reserved {
        mailbox_id,
        first_uid,
        base_modseq: new_modseq - i64::from(n),
    })
}
