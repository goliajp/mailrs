//! The fact ledger in SQL: the same records the kevy store keeps, found by
//! the same axes. A message's facts are inserted once and never updated;
//! its verdict is replaced at will.

use crate::ledger::{IndexedLink, LedgerFacts, LedgerVerdict};
use crate::pg::PgMailboxStore;

/// `name` is `domain` or a subdomain of it.
fn under(name: &str, domain: &str) -> bool {
    let (name, domain) = (name.to_ascii_lowercase(), domain.to_ascii_lowercase());
    name == domain || name.ends_with(&format!(".{domain}"))
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

impl PgMailboxStore {
    /// Records what a delivered message was, for its reader, with a link
    /// row for each of `indexed_hosts` kept until `links_until` when given
    /// (none at all when that has passed). Written once: a second call for
    /// the same reader and message leaves the first record and is false.
    pub async fn record_ledger_indexed(
        &self,
        f: &LedgerFacts,
        indexed_hosts: &[IndexedLink],
        links_until: Option<i64>,
    ) -> Result<bool, sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let indexed: &[IndexedLink] = match links_until {
            Some(until) if until <= now => &[],
            _ => indexed_hosts,
        };
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO ledger_messages (user_address, message_id, arrived, from_addr,
                    from_domain, from_reg, reply_to, auth, link_hosts, attachment_sha256)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_address, message_id) DO NOTHING",
        )
        .bind(&f.user)
        .bind(&f.message_id)
        .bind(f.arrived)
        .bind(&f.from_addr)
        .bind(&f.from_domain)
        .bind(&f.from_reg)
        .bind(&f.reply_to)
        .bind(&f.auth)
        .bind(f.link_hosts.join(" "))
        .bind(f.attachment_sha256.join(" "))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Ok(false);
        }
        for link in indexed {
            sqlx::query(
                "INSERT INTO ledger_links (user_address, message_id, host, reg, until)
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            )
            .bind(&f.user)
            .bind(&f.message_id)
            .bind(&link.host)
            .bind(&link.reg)
            .bind(links_until)
            .execute(&mut *tx)
            .await?;
        }
        for sha in &f.attachment_sha256 {
            sqlx::query(
                "INSERT INTO ledger_files (user_address, message_id, sha256)
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(&f.user)
            .bind(&f.message_id)
            .bind(sha)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Whether `user`'s copy of a message is in the ledger.
    pub async fn ledger_has(&self, user: &str, mid: &str) -> Result<bool, sqlx::Error> {
        let found: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM ledger_messages WHERE user_address = $1 AND message_id = $2",
        )
        .bind(user)
        .bind(mid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(found.is_some())
    }

    /// Replaces the verdict on `user`'s copy of a message.
    pub async fn record_verdict(
        &self,
        user: &str,
        mid: &str,
        v: &LedgerVerdict,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO ledger_verdicts (user_address, message_id, rules, score,
                    rules_version, judged_at, holds, screening)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (user_address, message_id) DO UPDATE SET
                rules = EXCLUDED.rules, score = EXCLUDED.score,
                rules_version = EXCLUDED.rules_version, judged_at = EXCLUDED.judged_at,
                holds = EXCLUDED.holds, screening = EXCLUDED.screening",
        )
        .bind(user)
        .bind(mid)
        .bind(v.rules.join(" "))
        .bind(v.score)
        .bind(&v.rules_version)
        .bind(v.judged_at)
        .bind(v.holds)
        .bind(v.screening.join(" "))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The verdict last recorded on `user`'s copy of a message.
    pub async fn ledger_verdict(
        &self,
        user: &str,
        mid: &str,
    ) -> Result<Option<LedgerVerdict>, sqlx::Error> {
        type Row = (String, f64, String, i64, bool, String);
        let row: Option<Row> = sqlx::query_as(
            "SELECT rules, score, rules_version, judged_at, holds, screening
               FROM ledger_verdicts WHERE user_address = $1 AND message_id = $2",
        )
        .bind(user)
        .bind(mid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(rules, score, rules_version, judged_at, holds, screening)| LedgerVerdict {
                rules: words(&rules),
                score,
                rules_version,
                judged_at,
                holds,
                screening: words(&screening),
            },
        ))
    }

    /// The messages from `domain` or any subdomain of it, as `(message id,
    /// user)`; `reg` is `domain`'s registrable domain, which every row
    /// under it is found by.
    pub async fn ledger_by_domain_tree(
        &self,
        domain: &str,
        reg: &str,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT message_id, user_address, from_domain FROM ledger_messages
              WHERE from_reg = $1",
        )
        .bind(reg)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter(|(_, _, from)| under(from, domain))
            .map(|(mid, user, _)| (mid, user))
            .collect())
    }

    /// The messages with a link row for `host` or any subdomain of it, as
    /// `(message id, user)`, once each; `reg` as for
    /// [`Self::ledger_by_domain_tree`]. Only what the link retention keeps.
    pub async fn ledger_by_link_tree(
        &self,
        host: &str,
        reg: &str,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT message_id, user_address, host FROM ledger_links WHERE reg = $1",
        )
        .bind(reg)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<(String, String)> = Vec::new();
        for (mid, user, linked) in rows {
            if under(&linked, host) && !out.iter().any(|(m, u)| *m == mid && *u == user) {
                out.push((mid, user));
            }
        }
        Ok(out)
    }

    /// Delete the link rows kept until `now` or earlier; how many.
    pub async fn prune_expired_links(&self, now: i64) -> Result<u64, sqlx::Error> {
        let done = sqlx::query("DELETE FROM ledger_links WHERE until IS NOT NULL AND until <= $1")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// Forgets `user`'s copy of a message: its facts, their rows and its
    /// verdict.
    pub async fn delete_ledger(&self, user: &str, mid: &str) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for table in [
            "ledger_links",
            "ledger_files",
            "ledger_verdicts",
            "ledger_messages",
        ] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE user_address = $1 AND message_id = $2"
            ))
            .bind(user)
            .bind(mid)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    /// After a delete: the ledger of each of `mids` that `user` no longer
    /// has a copy of is forgotten, as the kevy store forgets it with the
    /// message. A copy left in another folder keeps it.
    pub(crate) async fn forget_ledger_of_gone(
        &self,
        user: &str,
        mids: &[String],
    ) -> Result<(), sqlx::Error> {
        for mid in mids {
            let kept: Option<(i32,)> = sqlx::query_as(
                "SELECT 1 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                  WHERE b.user_address = $1 AND m.message_id = $2 LIMIT 1",
            )
            .bind(user)
            .bind(mid)
            .fetch_optional(&self.pool)
            .await?;
            if kept.is_none() {
                self.delete_ledger(user, mid).await?;
            }
        }
        Ok(())
    }

    /// [`Self::forget_ledger_of_gone`] for the owner of mailbox
    /// `mailbox_id`.
    pub(crate) async fn forget_ledger_in(
        &self,
        mailbox_id: i64,
        mids: &[String],
    ) -> Result<(), sqlx::Error> {
        if mids.is_empty() {
            return Ok(());
        }
        let owner: Option<(String,)> =
            sqlx::query_as("SELECT user_address FROM mailboxes WHERE id = $1")
                .bind(mailbox_id)
                .fetch_optional(&self.pool)
                .await?;
        match owner {
            Some((user,)) => self.forget_ledger_of_gone(&user, mids).await,
            None => Ok(()),
        }
    }

    /// Up to `limit` messages after row `after`, in row order, outside a
    /// Sent folder and with no ledger record for their reader: `(row id,
    /// reader, Message-ID, maildir file)`.
    pub async fn unledgered(
        &self,
        after: i64,
        limit: i64,
    ) -> Result<Vec<(i64, String, String, String)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT m.id, b.user_address, m.message_id, m.maildir_id
               FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
              WHERE m.id > $1 AND b.name <> 'Sent'
                AND NOT EXISTS (SELECT 1 FROM ledger_messages l
                                 WHERE l.user_address = b.user_address
                                   AND l.message_id = m.message_id)
              ORDER BY m.id LIMIT $2",
        )
        .bind(after)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
}
