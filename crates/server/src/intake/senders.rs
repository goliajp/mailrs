//! A reader's rules for one sender, on the SQL store
//! (`mailrs_intake::sender_ops` decides; this changes the rows).

use std::io;

use mailrs_core_sidestate::families::sender_rules::Disposal;
use mailrs_intake::sender_ops::{SenderStore, SenderThread};
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::scope::bucket_of;

use super::PgCore;

impl SenderStore for PgCore {
    fn sender_threads(&self, user: &str, sender: &str) -> io::Result<Vec<SenderThread>> {
        let sender = sender.trim().to_ascii_lowercase();
        let q = ThreadQuery {
            archived: None,
            trashed: None,
            held: Held::Include,
            in_from: Some(&sender),
            ..ThreadQuery::all(user)
        };
        let rows = self
            .dir
            .rt
            .block_on(self.mailbox.thread_rows(&q))
            .map_err(io::Error::other)?;
        Ok(rows
            .into_iter()
            .map(|r| SenderThread {
                bucket: bucket_of(&r.category).name().to_string(),
                latest_date: r.last_date,
                archived: r.archived,
                trashed: r.trashed_at > 0,
                held: r.quarantined,
                starred: r.flagged,
                pinned: r.pinned,
                thread_id: r.thread_id,
            })
            .collect())
    }

    fn set_bucket(&self, user: &str, thread_id: &str, bucket: &str) -> io::Result<()> {
        let category = match bucket {
            "inbox" => "inbox",
            "notifications" => "notification",
            "promotions" => "promotion",
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("not a bucket a sender's mail can go to: {other}"),
                ));
            }
        };
        self.dir
            .rt
            .block_on(self.mailbox.set_thread_bucket(user, thread_id, category))
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn dispose(&self, user: &str, thread_id: &str, how: Disposal, now: i64) -> io::Result<()> {
        let done = match how {
            Disposal::Archive => self
                .dir
                .rt
                .block_on(self.mailbox.archive_thread(user, thread_id))
                .map(|_| ()),
            Disposal::Trash => self
                .dir
                .rt
                .block_on(self.mailbox.trash_thread(user, thread_id, now)),
        };
        done.map_err(io::Error::other)
    }
}
