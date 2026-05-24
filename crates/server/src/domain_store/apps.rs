//! Third-party app / API-key registration handlers.

use super::{App, DomainStore, Result};

impl DomainStore {
    /// list all apps, optionally filtered by owner
    pub async fn list_apps(&self, owner: Option<&str>) -> Result<Vec<App>> {
        let pool = self.pg()?;
        let rows = if let Some(owner_addr) = owner {
            sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
                "SELECT id, app_id, name, description, owner_address, scopes, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM apps WHERE owner_address = $1 ORDER BY name",
            )
            .bind(owner_addr)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
                "SELECT id, app_id, name, description, owner_address, scopes, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint \
                 FROM apps ORDER BY name",
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(id, app_id, name, description, owner_address, scopes, active, created_at)| App {
                    id,
                    app_id,
                    name,
                    description,
                    owner_address,
                    scopes,
                    active,
                    created_at,
                },
            )
            .collect())
    }

    /// create a new app, returns the internal id
    pub async fn create_app(
        &self,
        app_id: &str,
        name: &str,
        description: &str,
        owner_address: &str,
        scopes: &str,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let (id,) = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO apps (app_id, name, description, owner_address, scopes) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(app_id)
        .bind(name)
        .bind(description)
        .bind(owner_address)
        .bind(scopes)
        .fetch_one(pool)
        .await?;
        Ok(id)
    }

    /// get an app by app_id
    pub async fn get_app(&self, app_id: &str) -> Result<Option<App>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
            "SELECT id, app_id, name, description, owner_address, scopes, active, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM apps WHERE app_id = $1",
        )
        .bind(app_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(
            |(id, app_id, name, description, owner_address, scopes, active, created_at)| App {
                id,
                app_id,
                name,
                description,
                owner_address,
                scopes,
                active,
                created_at,
            },
        ))
    }

    /// get an app by internal id
    pub async fn get_app_by_id(&self, id: i64) -> Result<Option<App>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64)>(
            "SELECT id, app_id, name, description, owner_address, scopes, active, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM apps WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(
            |(id, app_id, name, description, owner_address, scopes, active, created_at)| App {
                id,
                app_id,
                name,
                description,
                owner_address,
                scopes,
                active,
                created_at,
            },
        ))
    }

    /// remove an app (cascades to its api_keys)
    pub async fn remove_app(&self, app_id: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM apps WHERE app_id = $1")
            .bind(app_id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// update app scopes
    pub async fn update_app_scopes(&self, app_id: &str, scopes: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("UPDATE apps SET scopes = $1 WHERE app_id = $2")
            .bind(scopes)
            .bind(app_id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
