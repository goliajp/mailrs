//! A reader's labels: `labels` holds the definitions, `thread_labels` which
//! conversations each is on.

use std::collections::HashMap;

use crate::labels::{Label, LabelError, label_color, label_name, name_taken};
use crate::pg::PgMailboxStore;

/// A label operation refused, or the database failing.
#[derive(Debug, thiserror::Error)]
pub enum LabelDbError {
    /// The rules every core applies refused it.
    #[error(transparent)]
    Label(#[from] LabelError),
    /// The database failed.
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

type Row = (i64, String, String, i64);

fn label((id, name, color, created_at): Row) -> Label {
    Label {
        id: id.to_string(),
        name,
        color,
        created_at,
    }
}

impl PgMailboxStore {
    /// `user`'s labels, by name.
    pub async fn labels(&self, user: &str) -> Result<Vec<Label>, sqlx::Error> {
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, name, color, created_at FROM labels WHERE account_address = $1",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<Label> = rows.into_iter().map(label).collect();
        out.sort_by_key(|l| l.name.to_lowercase());
        Ok(out)
    }

    /// A new label; ids count up per user from 1, as the kevy store's do.
    pub async fn create_label(
        &self,
        user: &str,
        name: &str,
        color: &str,
        now: i64,
    ) -> Result<Label, LabelDbError> {
        let name = label_name(name)?;
        let color = label_color(color)?;
        if name_taken(&self.labels(user).await?, &name, None) {
            return Err(LabelError::Taken(name).into());
        }
        let row: Row = sqlx::query_as(
            "INSERT INTO labels (account_address, id, name, color, created_at)
             SELECT $1, COALESCE(MAX(id), 0) + 1, $2, $3, $4 FROM labels WHERE account_address = $1
             RETURNING id, name, color, created_at",
        )
        .bind(user)
        .bind(&name)
        .bind(&color)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(label(row))
    }

    /// Rename or recolour; the id and what it is on stay.
    pub async fn update_label(
        &self,
        user: &str,
        id: &str,
        name: Option<&str>,
        color: Option<&str>,
    ) -> Result<Label, LabelDbError> {
        let labels = self.labels(user).await?;
        let mut l = labels
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .ok_or(LabelError::NotFound)?;
        if let Some(n) = name {
            let n = label_name(n)?;
            if name_taken(&labels, &n, Some(id)) {
                return Err(LabelError::Taken(n).into());
            }
            l.name = n;
        }
        if let Some(c) = color {
            l.color = label_color(c)?;
        }
        sqlx::query(
            "UPDATE labels SET name = $3, color = $4 WHERE account_address = $1 AND id = $2",
        )
        .bind(user)
        .bind(label_id(id)?)
        .bind(&l.name)
        .bind(&l.color)
        .execute(&self.pool)
        .await?;
        Ok(l)
    }

    /// The label and its applications; the mail stays. `false` when there
    /// was no such label.
    pub async fn delete_label(&self, user: &str, id: &str) -> Result<bool, LabelDbError> {
        let id = label_id(id)?;
        sqlx::query("DELETE FROM thread_labels WHERE account_address = $1 AND label_id = $2")
            .bind(user)
            .bind(id)
            .execute(&self.pool)
            .await?;
        let gone = sqlx::query("DELETE FROM labels WHERE account_address = $1 AND id = $2")
            .bind(user)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(gone.rows_affected() > 0)
    }

    /// Put label `id` on `thread_id`, or take it off. The caller checks the
    /// user is in the conversation.
    pub async fn set_label(
        &self,
        user: &str,
        id: &str,
        thread_id: &str,
        on: bool,
    ) -> Result<(), LabelDbError> {
        if !self.labels(user).await?.iter().any(|l| l.id == id) {
            return Err(LabelError::NotFound.into());
        }
        let sql = match on {
            true => {
                "INSERT INTO thread_labels (account_address, label_id, thread_id) VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING"
            }
            false => {
                "DELETE FROM thread_labels WHERE account_address = $1 AND label_id = $2 AND thread_id = $3"
            }
        };
        sqlx::query(sql)
            .bind(user)
            .bind(label_id(id)?)
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The conversations label `id` is on.
    pub async fn threads_with_label(
        &self,
        user: &str,
        id: &str,
    ) -> Result<Vec<String>, LabelDbError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT thread_id FROM thread_labels WHERE account_address = $1 AND label_id = $2",
        )
        .bind(user)
        .bind(label_id(id)?)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(t,)| t).collect())
    }

    /// Every labelled conversation of `user` and its label ids.
    pub async fn labels_by_thread(
        &self,
        user: &str,
    ) -> Result<HashMap<String, Vec<String>>, sqlx::Error> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT tl.thread_id, tl.label_id FROM thread_labels tl
               JOIN labels l ON l.account_address = tl.account_address AND l.id = tl.label_id
              WHERE tl.account_address = $1
              ORDER BY LOWER(l.name)",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for (tid, id) in rows {
            out.entry(tid).or_default().push(id.to_string());
        }
        Ok(out)
    }

    /// A conversation deleted for good leaves every label of `user`.
    pub async fn drop_thread_labels(&self, user: &str, thread_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM thread_labels WHERE account_address = $1 AND thread_id = $2")
            .bind(user)
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Ids are decimal on the wire; anything else names no label.
fn label_id(id: &str) -> Result<i64, LabelError> {
    id.parse().map_err(|_| LabelError::NotFound)
}
