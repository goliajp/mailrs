//! This core's part of purging a deleted account or group: its rows, by
//! table. The flow is `mailrs_core_routes::purge`.

use std::sync::Arc;

use mailrs_core_api::method::admin::PurgeCount;
use mailrs_core_routes::store::{PurgeStore, StoreError, StoreResult};
use mailrs_core_sidestate::families::account_purge::registry::{SQL_CASCADED, SQL_TABLES};

use super::CoreRpcState;

fn failed(e: sqlx::Error) -> StoreError {
    StoreError(format!("the database: {e}"))
}

/// A message's template fingerprint is kept once per message, not per
/// reader: the account's purge takes those of the messages it is the last
/// reader of, as fastcore's does in its store. Run before the mailboxes go.
const LAST_READER_TEMPLATES: &str = "FROM message_templates t \
     WHERE t.message_id IN (SELECT m.message_id FROM messages m \
         JOIN mailboxes b ON b.id = m.mailbox_id WHERE b.user_address = $1) \
     AND NOT EXISTS (SELECT 1 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
         WHERE m.message_id = t.message_id AND b.user_address <> $1)";

/// The table a group purge counts this core's reply claims under.
const REPLY_TABLE: &str = "shared_reply_state";

pub(crate) async fn is_account(state: &CoreRpcState, address: &str) -> StoreResult<bool> {
    let found: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM accounts WHERE address = $1 AND deleted_at IS NULL")
            .bind(address)
            .fetch_optional(&state.pool)
            .await
            .map_err(failed)?;
    Ok(found.is_some())
}

async fn count(state: &CoreRpcState, q: &str, address: &str) -> StoreResult<u64> {
    let (n,): (i64,) = sqlx::query_as(q)
        .bind(address)
        .fetch_one(&state.pool)
        .await
        .map_err(failed)?;
    Ok(n as u64)
}

impl PurgeStore for CoreRpcState {
    /// The account's rows, by table, and its messages, which go with its
    /// mailboxes.
    async fn count_mail(self: &Arc<Self>, address: &str) -> StoreResult<Vec<PurgeCount>> {
        let mut queries = vec![
            (
                "message_templates".to_string(),
                format!("SELECT COUNT(*) {LAST_READER_TEMPLATES}"),
            ),
            (
                "messages".to_string(),
                "SELECT COUNT(*) FROM messages WHERE mailbox_id IN \
                 (SELECT id FROM mailboxes WHERE user_address = $1)"
                    .to_string(),
            ),
        ];
        let rows = SQL_TABLES
            .iter()
            .chain(SQL_CASCADED)
            .chain(&[("accounts", "address")]);
        for (table, column) in rows {
            queries.push((
                table.to_string(),
                format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1"),
            ));
        }
        let mut out = Vec::new();
        for (name, q) in queries {
            let n = count(self, &q, address).await?;
            if n > 0 {
                out.push(PurgeCount { name, count: n });
            }
        }
        Ok(out)
    }

    async fn remove_mail(self: &Arc<Self>, address: &str) -> StoreResult<()> {
        let mut tx = self.pool.begin().await.map_err(failed)?;
        sqlx::query(&format!("DELETE {LAST_READER_TEMPLATES}"))
            .bind(address)
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
        for (table, column) in SQL_TABLES {
            let q = format!("DELETE FROM {table} WHERE {column} = $1");
            sqlx::query(&q)
                .bind(address)
                .execute(&mut *tx)
                .await
                .map_err(failed)?;
        }
        // the row last: what references it goes with it
        sqlx::query("DELETE FROM accounts WHERE address = $1 AND deleted_at IS NOT NULL")
            .bind(address)
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
        tx.commit().await.map_err(failed)?;
        // the re-judge's marks and gathered conversations of this reader
        let core = self.pg_core();
        let address = address.to_string();
        tokio::task::spawn_blocking(move || mailrs_intake::rejudge::forget_reader(&core, &address))
            .await
            .map_err(|e| StoreError(e.to_string()))??;
        Ok(())
    }

    /// Every table this core keeps for an account is in the registry.
    async fn mail_unregistered(self: &Arc<Self>, _address: &str) -> StoreResult<Vec<PurgeCount>> {
        Ok(Vec::new())
    }

    async fn purge_group_replies(
        self: &Arc<Self>,
        gid: i64,
        dry_run: bool,
    ) -> StoreResult<PurgeCount> {
        let n = self
            .mailbox
            .purge_group_replies(gid, dry_run)
            .await
            .map_err(failed)?;
        Ok(PurgeCount {
            name: REPLY_TABLE.to_string(),
            count: n,
        })
    }
}
